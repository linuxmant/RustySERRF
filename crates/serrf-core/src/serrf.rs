use crate::correlation::{select_variables, spearman_corr_matrix};
use crate::forest::{default_mtry, ForestConfig, RandomForest};
use ndarray::{Array2, ArrayView2, Axis};
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

pub struct GroupInput<'a> {
    pub train: ArrayView2<'a, f64>,
    pub target: ArrayView2<'a, f64>,
    pub train_batch: &'a [String],
    pub target_batch: &'a [String],
    pub num_vars: usize,
}

pub struct GroupOutput {
    pub normed_train: Array2<f64>,
    pub normed_target: Array2<f64>,
}

pub fn serrf_normalize_group(input: &GroupInput, seed: u64, mut progress: impl FnMut(usize, usize)) -> GroupOutput {
    let n_compounds = input.train.nrows();
    let n_train = input.train.ncols();
    let n_target = input.target.ncols();

    // combined matrix: QC columns first, then target columns, matching serrfR's `all = cbind(train, target)`
    let mut all = Array2::<f64>::zeros((n_compounds, n_train + n_target));
    all.slice_mut(ndarray::s![.., 0..n_train]).assign(&input.train);
    all.slice_mut(ndarray::s![.., n_train..]).assign(&input.target);
    let is_qc: Vec<bool> = (0..n_train + n_target).map(|i| i < n_train).collect();
    let batch: Vec<String> = input.train_batch.iter().chain(input.target_batch.iter()).cloned().collect();
    let batches: Vec<String> = {
        let mut seen = Vec::new();
        for b in &batch {
            if !seen.contains(b) {
                seen.push(b.clone());
            }
        }
        seen
    };

    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // fix zeros/NAs per batch with small jitter, mirroring serrfR lines 413-421
    for b in &batches {
        let cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b).collect();
        for i in 0..n_compounds {
            let nonzero_nonnan_min = cols
                .iter()
                .map(|&c| all[[i, c]])
                .filter(|v| !v.is_nan() && *v != 0.0)
                .fold(f64::INFINITY, f64::min);
            // If this compound has no non-zero, non-NaN value anywhere in this batch (e.g. it
            // is entirely zero, or entirely NaN, within this one batch), there is no sensible
            // basis for the jitter fix and `nonzero_nonnan_min` folds to `f64::INFINITY`.
            // Skip the jitter here rather than writing `INFINITY + jitter` back into `all`,
            // which would otherwise cascade into NaN via mean/median arithmetic downstream.
            // The zeros/NaNs are left as-is; the per-batch non-finite rescue below (mirroring
            // app.R:600) and the final `median()`'s `total_cmp` are the remaining safety nets.
            if !nonzero_nonnan_min.is_finite() {
                continue;
            }
            for &c in &cols {
                if all[[i, c]] == 0.0 {
                    all[[i, c]] = nonzero_nonnan_min + 1.0 + rng.gen_range(-0.1..0.1);
                } else if all[[i, c]].is_nan() {
                    all[[i, c]] = 0.5 * nonzero_nonnan_min + 1.0 + rng.gen_range(-0.1..0.1);
                }
            }
        }
    }

    // per-batch correlation matrices among QC and among target, mirroring serrfR lines 425-443
    let mut corr_train: HashMap<String, Array2<f64>> = HashMap::new();
    let mut corr_target: HashMap<String, Array2<f64>> = HashMap::new();
    for b in &batches {
        let qc_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b && is_qc[c]).collect();
        let target_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b && !is_qc[c]).collect();
        corr_train.insert(b.clone(), spearman_corr_matrix(&all.select(Axis(1), &qc_cols)));
        corr_target.insert(b.clone(), spearman_corr_matrix(&all.select(Axis(1), &target_cols)));
    }

    let mut normalized = Array2::<f64>::zeros((n_compounds, all.ncols()));

    for j in 0..n_compounds {
        progress(j + 1, n_compounds);
        let mut row_normalized = vec![0.0; all.ncols()];

        // whole-group (all batches) QC mean and target median for compound j, matching
        // serrfR's `mean(all[j,sampleType.=='qc'])` (line 571) and
        // `median(all[j,!sampleType.=='qc'])` (line 576) rescale denominators.
        let all_qc_indices: Vec<usize> = (0..all.ncols()).filter(|&c| is_qc[c]).collect();
        let all_target_indices: Vec<usize> = (0..all.ncols()).filter(|&c| !is_qc[c]).collect();
        let overall_qc_mean = all_qc_indices.iter().map(|&c| all[[j, c]]).sum::<f64>() / all_qc_indices.len() as f64;
        let overall_target_median = {
            let mut vals: Vec<f64> = all_target_indices.iter().map(|&c| all[[j, c]]).collect();
            median(&mut vals)
        };

        for b in &batches {
            let batch_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b).collect();
            let qc_cols: Vec<usize> = batch_cols.iter().copied().filter(|&c| is_qc[c]).collect();
            let target_cols: Vec<usize> = batch_cols.iter().copied().filter(|&c| !is_qc[c]).collect();

            let selected = select_variables(&corr_train[b], &corr_target[b], j, input.num_vars);
            if selected.is_empty() {
                for &c in &batch_cols {
                    row_normalized[c] = all[[j, c]];
                }
            } else {
                let train_y: Vec<f64> = qc_cols.iter().map(|&c| all[[j, c]]).collect();
                let train_y_mean = train_y.iter().sum::<f64>() / train_y.len() as f64;
                let centered_train_y: Vec<f64> = train_y.iter().map(|v| v - train_y_mean).collect();

                let train_x: Vec<Vec<f64>> = qc_cols
                    .iter()
                    .map(|&c| selected.iter().map(|&i| all[[i, c]]).collect())
                    .collect();
                let test_x: Vec<Vec<f64>> = target_cols
                    .iter()
                    .map(|&c| selected.iter().map(|&i| all[[i, c]]).collect())
                    .collect();

                // per-(compound, batch) seed so RF randomness isn't perfectly correlated across
                // every compound/batch in the run (I3): distinct but deterministic given the
                // same (seed, compound_index, batch).
                let forest_config = ForestConfig {
                    num_trees: 500,
                    mtry: default_mtry(selected.len()),
                    min_node_size: 5,
                    seed: derive_forest_seed(seed, j, b),
                };
                let forest = RandomForest::train(&train_x, &centered_train_y, &forest_config);

                // ratio denominator is the OVERALL (whole-group) QC mean, not the batch-local one:
                // serrfR line 571 divides by `mean(all[j,sampleType.=='qc'])`, which is computed
                // across all batches. Using the batch-local mean here instead would make the ratio
                // collapse to ~1 for every batch and silently defeat the batch-drift correction.
                for (idx, &c) in qc_cols.iter().enumerate() {
                    let predicted = forest.predict(&train_x[idx]) + train_y_mean;
                    let ratio = predicted / overall_qc_mean;
                    row_normalized[c] = if ratio.abs() < 1e-9 { all[[j, c]] } else { all[[j, c]] / ratio };
                }

                let target_values: Vec<f64> = target_cols.iter().map(|&c| all[[j, c]]).collect();
                let target_mean = target_values.iter().sum::<f64>() / target_values.len().max(1) as f64;
                let predictions: Vec<f64> = test_x.iter().map(|row| forest.predict(row)).collect();
                let prediction_mean = if predictions.is_empty() { 0.0 } else { predictions.iter().sum::<f64>() / predictions.len() as f64 };
                // ratio denominator is the OVERALL (whole-group) target median, matching serrfR
                // line 576's `median(all[j,!sampleType.=='qc'])`, for the same reason as above.
                for (idx, &c) in target_cols.iter().enumerate() {
                    let predicted = predictions[idx] + target_mean - prediction_mean;
                    let ratio = predicted / overall_target_median;
                    row_normalized[c] = if ratio.abs() < 1e-9 { all[[j, c]] } else { all[[j, c]] / ratio };
                }

                // negative-value fix: fall back to the raw value (serrfR line 588/622)
                for &c in &target_cols {
                    if row_normalized[c] < 0.0 {
                        row_normalized[c] = all[[j, c]];
                    }
                }
            }

            // fix non-finite values within this batch, mirroring app.R:600's
            // `norm[!is.finite(norm)] = rnorm(..., sd = sd(norm[is.finite(norm)])*0.01)`.
            // Runs regardless of which branch above populated `row_normalized` for this batch,
            // so it also catches the zero/NaN entries the jitter-loop guard above deliberately
            // left untouched. If every value in the batch is non-finite there is no spread to
            // scale noise from, so fall back to the raw value instead (this port's "statistical
            // equivalence, not exact match" philosophy rather than R's exact `rnorm` call).
            let batch_finite: Vec<f64> = batch_cols.iter().map(|&c| row_normalized[c]).filter(|v| v.is_finite()).collect();
            if batch_finite.len() < batch_cols.len() {
                if batch_finite.is_empty() {
                    // Falling back to the raw value is only safe when the raw value is itself
                    // finite. A single non-finite raw cell can poison the whole batch's forest
                    // (it trains on that batch's own y, so one Inf/NaN QC value can make every
                    // prediction in the batch NaN) — in that case the raw fallback would just
                    // reintroduce the original non-finite cell, so fall back to 0.0 instead for
                    // any column whose raw value is also non-finite.
                    for &c in &batch_cols {
                        row_normalized[c] = if all[[j, c]].is_finite() { all[[j, c]] } else { 0.0 };
                    }
                } else {
                    let noise_scale = sample_std_dev(&batch_finite) * 0.01;
                    for &c in &batch_cols {
                        if !row_normalized[c].is_finite() {
                            row_normalized[c] = noise_scale * rng.gen_range(-1.0..1.0);
                        }
                    }
                }
            }
        }

        // final rescale of QC and non-QC groups to the original overall medians (serrfR lines 594-597)
        rescale_to_median(&mut row_normalized, &all_qc_indices, &all.row(j).to_vec());
        rescale_to_median(&mut row_normalized, &all_target_indices, &all.row(j).to_vec());

        for c in 0..all.ncols() {
            normalized[[j, c]] = row_normalized[c];
        }
    }

    GroupOutput {
        normed_train: normalized.slice(ndarray::s![.., 0..n_train]).to_owned(),
        normed_target: normalized.slice(ndarray::s![.., n_train..]).to_owned(),
    }
}

fn rescale_to_median(row: &mut [f64], indices: &[usize], original: &[f64]) {
    let mut normed_vals: Vec<f64> = indices.iter().map(|&i| row[i]).collect();
    let mut orig_vals: Vec<f64> = indices.iter().map(|&i| original[i]).collect();
    let normed_median = median(&mut normed_vals);
    let orig_median = median(&mut orig_vals);
    if normed_median.abs() > 1e-9 {
        let factor = orig_median / normed_median;
        for &i in indices {
            row[i] *= factor;
        }
    }
}

fn median(values: &mut [f64]) -> f64 {
    // total_cmp (not partial_cmp().unwrap()) so a NaN/Inf value that reaches this sort can
    // never panic (C1 defense-in-depth, on top of the fixes upstream that avoid manufacturing
    // non-finite values in the first place).
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 0 { (values[n / 2 - 1] + values[n / 2]) / 2.0 } else { values[n / 2] }
}

/// Sample standard deviation (n-1 denominator), used only to scale the non-finite-value rescue
/// noise below; returns 0.0 for fewer than 2 values rather than dividing by zero.
fn sample_std_dev(values: &[f64]) -> f64 {
    let n = values.len();
    if n < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    variance.sqrt()
}

/// Derives a per-(compound, batch) random forest seed from the group seed so RF randomness is
/// not perfectly correlated across every compound/batch in a run (I3), while staying
/// deterministic: the same (seed, compound_index, batch) always hashes to the same u64.
fn derive_forest_seed(seed: u64, compound_index: usize, batch: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    compound_index.hash(&mut hasher);
    batch.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    /// Builds a synthetic dataset with an engineered per-batch additive drift on QC samples,
    /// which SERRF should correct so post-normalization QC RSD drops relative to raw QC RSD.
    fn synthetic_group() -> (Array2<f64>, Array2<f64>, Vec<String>, Vec<String>) {
        let n_compounds = 6;
        let n_qc = 12;
        let n_sample = 8;
        let mut train = Array2::<f64>::zeros((n_compounds, n_qc));
        let mut target = Array2::<f64>::zeros((n_compounds, n_sample));
        let mut train_batch = Vec::new();
        let mut target_batch = Vec::new();
        for j in 0..n_qc {
            let batch = if j < n_qc / 2 { "A" } else { "B" };
            train_batch.push(batch.to_string());
            let drift = if batch == "A" { 0.0 } else { 5.0 };
            for i in 0..n_compounds {
                train[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        for j in 0..n_sample {
            let batch = if j < n_sample / 2 { "A" } else { "B" };
            target_batch.push(batch.to_string());
            let drift = if batch == "A" { 0.0 } else { 5.0 };
            for i in 0..n_compounds {
                target[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        (train, target, train_batch, target_batch)
    }

    #[test]
    fn reduces_qc_rsd_relative_to_raw_when_batch_drift_is_present() {
        let (train, target, train_batch, target_batch) = synthetic_group();
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        let raw_rsd = crate::rsd::rsd(&train.row(0).to_vec());
        let normed_rsd = crate::rsd::rsd(&output.normed_train.row(0).to_vec());
        assert!(normed_rsd <= raw_rsd, "expected SERRF to not worsen QC RSD: raw={raw_rsd}, normed={normed_rsd}");
        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
    }

    // C1 regression tests: the five scenarios the final-review probe reproduced as panics
    // (serrf.rs:184, `median()`'s `partial_cmp().unwrap()`). Each constructs the smallest
    // fixture that reaches the crash site and asserts `serrf_normalize_group` completes without
    // panicking; where the scenario doesn't corrupt the whole compound row, output values are
    // also asserted finite.

    #[test]
    fn does_not_panic_when_a_compound_is_entirely_zero_within_one_batch() {
        let (mut train, mut target, train_batch, target_batch) = synthetic_group();
        // compound 0 is undetected (all zero) in every sample of batch A, but normal in batch B.
        for j in 0..train_batch.len() {
            if train_batch[j] == "A" {
                train[[0, j]] = 0.0;
            }
        }
        for j in 0..target_batch.len() {
            if target_batch[j] == "A" {
                target[[0, j]] = 0.0;
            }
        }
        let input = GroupInput { train: train.view(), target: target.view(), train_batch: &train_batch, target_batch: &target_batch, num_vars: 3 };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()), "expected a finite output for an all-zero-in-one-batch compound");
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn does_not_panic_with_a_single_infinite_cell_anywhere() {
        let (mut train, target, train_batch, target_batch) = synthetic_group();
        train[[0, 0]] = f64::INFINITY;
        let input = GroupInput { train: train.view(), target: target.view(), train_batch: &train_batch, target_batch: &target_batch, num_vars: 3 };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()), "expected a single stray Inf cell to be rescued to a finite value");
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn does_not_panic_when_a_compound_row_is_entirely_infinite() {
        let (mut train, mut target, train_batch, target_batch) = synthetic_group();
        for j in 0..train.ncols() {
            train[[0, j]] = f64::INFINITY;
        }
        for j in 0..target.ncols() {
            target[[0, j]] = f64::INFINITY;
        }
        let input = GroupInput { train: train.view(), target: target.view(), train_batch: &train_batch, target_batch: &target_batch, num_vars: 3 };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        // No pipeline-level `extract_infinite_rows` strip happens at this layer (that lives in
        // `pipeline::normalize`, tested separately); the guarantee here is no panic, a correctly
        // shaped output, and — since the all-non-finite-batch rescue falls back to 0.0 rather
        // than the (also non-finite) raw value — a fully finite output too.
        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()));
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn does_not_panic_when_a_compound_row_is_entirely_missing() {
        let (mut train, mut target, train_batch, target_batch) = synthetic_group();
        for j in 0..train.ncols() {
            train[[0, j]] = f64::NAN;
        }
        for j in 0..target.ncols() {
            target[[0, j]] = f64::NAN;
        }
        let input = GroupInput { train: train.view(), target: target.view(), train_batch: &train_batch, target_batch: &target_batch, num_vars: 3 };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        // Same caveat as above about no pipeline-level strip happening at this layer; the
        // all-non-finite-batch rescue still yields a fully finite output for this compound.
        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()));
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn scattered_partial_zeros_still_normalize_without_panicking() {
        // Regression check: a few isolated non-detections (not an entire batch) must remain
        // finite and, as before this fix wave, still not worsen QC RSD.
        let (mut train, target, train_batch, target_batch) = synthetic_group();
        train[[0, 0]] = 0.0;
        train[[0, 7]] = 0.0;
        let input = GroupInput { train: train.view(), target: target.view(), train_batch: &train_batch, target_batch: &target_batch, num_vars: 3 };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()));
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }
}
