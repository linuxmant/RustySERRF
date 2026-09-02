use crate::correlation::{select_variables, spearman_corr_matrix};
use crate::forest::{default_mtry, ForestConfig, RandomForest};
use ndarray::{Array2, ArrayView2, Axis};
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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

pub fn serrf_normalize_group(input: &GroupInput, seed: u64, progress: impl FnMut(usize, usize) + Send) -> GroupOutput {
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

    // The per-compound loop below is embarrassingly parallel (each `j` only reads `all` and
    // writes its own row), and the RF ensemble training inside it is by far the dominant cost of
    // a full run (~5.4 minutes single-threaded on the real dataset). Drive it with rayon instead
    // of a sequential `for`. `progress` is called from whichever thread finishes a compound, in
    // completion order rather than compound order, so it's wrapped in a `Mutex` and reports a
    // monotonically increasing "N of total done" count (via `completed`) instead of `j + 1` —
    // `j` itself is no longer meaningful as a progress position once compounds can finish out of
    // order.
    let progress = Mutex::new(progress);
    let completed = AtomicUsize::new(0);
    let rows: Vec<Vec<f64>> = (0..n_compounds)
        .into_par_iter()
        .map(|j| {
            let row = compute_compound_row(j, &all, &is_qc, &batches, &batch, &corr_train, &corr_target, input.num_vars, seed);
            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            if let Ok(mut p) = progress.lock() {
                p(done, n_compounds);
            }
            row
        })
        .collect();

    let mut normalized = Array2::<f64>::zeros((n_compounds, all.ncols()));
    for (j, row) in rows.into_iter().enumerate() {
        for c in 0..all.ncols() {
            normalized[[j, c]] = row[c];
        }
    }

    GroupOutput {
        normed_train: normalized.slice(ndarray::s![.., 0..n_train]).to_owned(),
        normed_target: normalized.slice(ndarray::s![.., n_train..]).to_owned(),
    }
}

/// Computes one compound row's normalized values across every batch (the body of what was
/// previously the sequential per-compound loop in `serrf_normalize_group`, extracted so it can
/// run independently on a rayon worker thread per `j`). Owns its own RNG, seeded deterministically
/// from `(seed, j)` via `derive_row_seed` rather than sharing the group-level RNG, since threads
/// running concurrently can't share a single mutable `ChaCha8Rng` and still be deterministic.
#[allow(clippy::too_many_arguments)]
fn compute_compound_row(
    j: usize,
    all: &Array2<f64>,
    is_qc: &[bool],
    batches: &[String],
    batch: &[String],
    corr_train: &HashMap<String, Array2<f64>>,
    corr_target: &HashMap<String, Array2<f64>>,
    num_vars: usize,
    seed: u64,
) -> Vec<f64> {
    let mut rng = ChaCha8Rng::seed_from_u64(derive_row_seed(seed, j));
    let mut row_normalized = vec![0.0; all.ncols()];

    // whole-group (all batches) QC mean/median and target median for compound j, matching
    // serrfR's `mean(all[j,sampleType.=='qc'])` (line 571), `median(all[j,sampleType.=='qc'])`
    // (line 594's rescale target), and `median(all[j,!sampleType.=='qc'])` (lines 576/597/606's
    // rescale target and outlier-swap `attempt` term).
    let all_qc_indices: Vec<usize> = (0..all.ncols()).filter(|&c| is_qc[c]).collect();
    let all_target_indices: Vec<usize> = (0..all.ncols()).filter(|&c| !is_qc[c]).collect();
    let overall_qc_mean = all_qc_indices.iter().map(|&c| all[[j, c]]).sum::<f64>() / all_qc_indices.len() as f64;
    let overall_qc_median = {
        let mut vals: Vec<f64> = all_qc_indices.iter().map(|&c| all[[j, c]]).collect();
        median(&mut vals)
    };
    let overall_target_median = {
        let mut vals: Vec<f64> = all_target_indices.iter().map(|&c| all[[j, c]]).collect();
        median(&mut vals)
    };

    for b in batches {
        let batch_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b).collect();
        let qc_cols: Vec<usize> = batch_cols.iter().copied().filter(|&c| is_qc[c]).collect();
        let target_cols: Vec<usize> = batch_cols.iter().copied().filter(|&c| !is_qc[c]).collect();

        let selected = select_variables(&corr_train[b], &corr_target[b], j, num_vars);
        // Stashed only when the RF branch below runs, so the outlier-swap after the non-finite
        // fix (which needs the same predictions/target_mean the main correction used) knows
        // whether to run for this batch at all — R's outlier swap (604-617) is unreachable from
        // the raw-passthrough branch (534-536), just like the per-batch rescale below.
        let mut swap_inputs: Option<(Vec<f64>, f64)> = None;
        if selected.is_empty() {
            for &c in &batch_cols {
                row_normalized[c] = all[[j, c]];
            }
        } else {
            let train_y: Vec<f64> = qc_cols.iter().map(|&c| all[[j, c]]).collect();
            let train_y_mean = train_y.iter().sum::<f64>() / train_y.len() as f64;
            let centered_train_y: Vec<f64> = train_y.iter().map(|v| v - train_y_mean).collect();

            let train_x: Vec<Vec<f64>> = qc_cols.iter().map(|&c| selected.iter().map(|&i| all[[i, c]]).collect()).collect();
            let test_x: Vec<Vec<f64>> = target_cols.iter().map(|&c| selected.iter().map(|&i| all[[i, c]]).collect()).collect();

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
            let prediction_mean = if predictions.is_empty() {
                0.0
            } else {
                predictions.iter().sum::<f64>() / predictions.len() as f64
            };
            // ratio denominator is the OVERALL (whole-group) target median, matching serrfR
            // line 576's `median(all[j,!sampleType.=='qc'])`, for the same reason as above.
            for (idx, &c) in target_cols.iter().enumerate() {
                let predicted = predictions[idx] + target_mean - prediction_mean;
                let ratio = predicted / overall_target_median;
                row_normalized[c] = if ratio.abs() < 1e-9 { all[[j, c]] } else { all[[j, c]] / ratio };
            }

            // negative-value fix: fall back to the raw value (serrfR line 588)
            for &c in &target_cols {
                if row_normalized[c] < 0.0 {
                    row_normalized[c] = all[[j, c]];
                }
            }

            // per-batch median rescale (serrfR lines 594/597): each batch's own QC/target median
            // independently gets forced to match the whole-group raw median.
            rescale_batch_to_overall_median(&mut row_normalized, &qc_cols, overall_qc_median);
            rescale_batch_to_overall_median(&mut row_normalized, &target_cols, overall_target_median);

            swap_inputs = Some((predictions, target_mean));
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

        // outlier swap (serrfR lines 605-617), run after the non-finite fix like in app.R.
        // Only reachable for batches that took the RF branch above (raw-passthrough batches have
        // no `predictions`/`target_mean` to build an alternative correction from, matching R
        // where this code is unreachable from the `ncol(train_data)==1` early-return branch).
        if let Some((predictions, target_mean)) = swap_inputs {
            outlier_swap(
                &mut row_normalized,
                &batch_cols,
                &target_cols,
                &all.row(j).to_vec(),
                &predictions,
                target_mean,
                overall_target_median,
            );
            // negative-value re-fix, post-swap (serrfR line 622)
            for &c in &target_cols {
                if row_normalized[c] < 0.0 {
                    row_normalized[c] = all[[j, c]];
                }
            }
        }
    }

    // post-hoc QC rescale ("c factor", serrfR lines 656-658), run after the per-batch rescale
    // and outlier swap above like in app.R.
    apply_c_factor(&mut row_normalized, &all_qc_indices, &all_target_indices, &all.row(j).to_vec());

    row_normalized
}

/// Per-batch median rescale (app.R lines 594-597): forces `batch_indices`' own median (within
/// `row`) to match `overall_orig_median` — the raw median computed across *all* batches for this
/// compound, not this batch's raw median. R runs this once per batch, independently aligning each
/// batch's median to the same overall target; a single whole-group rescale done once after every
/// batch is not equivalent whenever there's more than one batch.
fn rescale_batch_to_overall_median(row: &mut [f64], batch_indices: &[usize], overall_orig_median: f64) {
    if batch_indices.is_empty() {
        return;
    }
    let mut batch_vals: Vec<f64> = batch_indices.iter().map(|&i| row[i]).collect();
    let batch_median = median(&mut batch_vals);
    if batch_median.abs() > 1e-9 {
        let factor = overall_orig_median / batch_median;
        for &i in batch_indices {
            row[i] *= factor;
        }
    }
}

/// Per-batch outlier swap (app.R lines 604-617), run after the per-batch median rescale and the
/// non-finite-value rescue. Detects outliers (`coef = 3`, wider than RSD's default 1.5) across
/// the *whole batch* (QC and target together) and, for any target-side value that landed in that
/// outlier set, computes an alternative "minus"-style correction (`attempt`) from the same RF
/// prediction the main "divide"-style correction used. The alternative is only substituted in if
/// doing so would move the outlier's mean closer to the rest of the batch — R's own comment notes
/// "this may not help deal with outlier effect", i.e. this is a heuristic, not a guarantee.
fn outlier_swap(row: &mut [f64], batch_cols: &[usize], target_cols: &[usize], raw: &[f64], predictions: &[f64], target_mean: f64, overall_target_median: f64) {
    let batch_norm: Vec<f64> = batch_cols.iter().map(|&c| row[c]).collect();
    if batch_norm.is_empty() {
        return;
    }
    let batch_mean = batch_norm.iter().sum::<f64>() / batch_norm.len() as f64;
    let out = crate::rsd::outlier_values(&batch_norm, 3.0);
    if out.is_empty() {
        return;
    }
    let out_mean = out.iter().sum::<f64>() / out.len() as f64;

    let mut attempts: Vec<(usize, f64)> = Vec::new();
    for (idx, &c) in target_cols.iter().enumerate() {
        if out.contains(&row[c]) {
            let attempt = raw[c] - (predictions[idx] + target_mean - overall_target_median);
            attempts.push((c, attempt));
        }
    }
    if attempts.is_empty() {
        return;
    }
    let attempt_mean = attempts.iter().map(|(_, v)| v).sum::<f64>() / attempts.len() as f64;

    let should_swap = if out_mean > batch_mean {
        attempt_mean < out_mean
    } else {
        attempt_mean > out_mean
    };
    if should_swap {
        for (c, v) in attempts {
            row[c] = v;
        }
    }
}

/// Post-hoc QC rescale ("c factor", app.R lines 656-658): blends the raw QC/target medians and
/// spread against the already-normalized target's median/spread into a single multiplier applied
/// to the normalized QC values only. Guards against non-positive and non-finite `c` (R's
/// `ifelse(c>0,c,1)` plus this port's stricter no-panic invariant: a NaN/Inf `c` must never
/// poison the row) by falling back to a factor of 1, i.e. leaving the QC values unchanged.
fn apply_c_factor(row: &mut [f64], qc_indices: &[usize], target_indices: &[usize], raw: &[f64]) {
    let mut normalized_target: Vec<f64> = target_indices.iter().map(|&i| row[i]).collect();
    let mut normalized_qc: Vec<f64> = qc_indices.iter().map(|&i| row[i]).collect();
    let mut raw_qc: Vec<f64> = qc_indices.iter().map(|&i| raw[i]).collect();
    let mut raw_target: Vec<f64> = target_indices.iter().map(|&i| raw[i]).collect();

    let normalized_target_median = median(&mut normalized_target);
    let normalized_qc_median = median(&mut normalized_qc);
    let raw_qc_median = median(&mut raw_qc);
    let raw_target_median = median(&mut raw_target);
    let raw_target_sd = sample_std_dev(&raw_target);
    let normalized_target_sd = sample_std_dev(&normalized_target);

    let c = (normalized_target_median + (raw_qc_median - raw_target_median) / raw_target_sd * normalized_target_sd) / normalized_qc_median;
    let factor = if c.is_finite() && c > 0.0 { c } else { 1.0 };
    for &i in qc_indices {
        row[i] *= factor;
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
    if n.is_multiple_of(2) {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    } else {
        values[n / 2]
    }
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

/// Derives a per-compound RNG seed for `compute_compound_row`'s non-finite-value rescue noise.
/// Each `j` runs on its own rayon worker thread and can't share the group-level RNG safely, so
/// every row needs its own independent-but-deterministic RNG; a distinct `"row"` tag keeps this
/// seed space from colliding with `derive_forest_seed`'s.
fn derive_row_seed(seed: u64, compound_index: usize) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    "row".hash(&mut hasher);
    compound_index.hash(&mut hasher);
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
    fn produces_identical_output_across_repeated_calls_with_the_same_seed() {
        // Guards the rayon-parallelized per-compound loop: completion order across threads is
        // nondeterministic, so output must not depend on it. Same input + same seed must always
        // produce bit-identical output, run repeatedly to make a scheduling-order dependency
        // more likely to surface instead of passing by luck on a single run.
        let (train, target, train_batch, target_batch) = synthetic_group();
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };

        let baseline = serrf_normalize_group(&input, 7, |_, _| {});
        for _ in 0..5 {
            let repeat = serrf_normalize_group(&input, 7, |_, _| {});
            assert_eq!(repeat.normed_train, baseline.normed_train);
            assert_eq!(repeat.normed_target, baseline.normed_target);
        }
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
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(
            output.normed_train.iter().all(|v| v.is_finite()),
            "expected a finite output for an all-zero-in-one-batch compound"
        );
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn does_not_panic_with_a_single_infinite_cell_anywhere() {
        let (mut train, target, train_batch, target_batch) = synthetic_group();
        train[[0, 0]] = f64::INFINITY;
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(
            output.normed_train.iter().all(|v| v.is_finite()),
            "expected a single stray Inf cell to be rescued to a finite value"
        );
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
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
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
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        // Same caveat as above about no pipeline-level strip happening at this layer; the
        // all-non-finite-batch rescue still yields a fully finite output for this compound.
        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()));
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }

    // I4 regression tests: app.R lines 655-658 apply a post-hoc "c factor" that rescales the
    // normalized QC values using a ratio of raw QC/target medians and standard deviations vs.
    // the normalized target's median/sd. This was never ported (documented gap from Task 16's
    // review). RSD is scale-invariant to a uniform per-row QC multiplier, so the golden RSD test
    // can't catch a missing c factor — these tests exercise `apply_c_factor` directly.

    #[test]
    fn apply_c_factor_scales_qc_values_when_normalized_sample_spread_differs_from_raw() {
        let raw = vec![10.0, 20.0, 30.0, 40.0, 100.0, 200.0, 300.0];
        let qc_indices = vec![0, 1, 2, 3];
        let target_indices = vec![4, 5, 6];
        // normalized QC unchanged from raw (median already 25, matching rescale's guarantee);
        // normalized sample keeps the raw median (200) but with half the raw spread (sd 50 vs 100).
        let mut row = vec![10.0, 20.0, 30.0, 40.0, 150.0, 200.0, 250.0];

        apply_c_factor(&mut row, &qc_indices, &target_indices, &raw);

        // c = (200 + (-175/100)*50) / 25 = 4.5
        let expected_qc: [f64; 4] = [45.0, 90.0, 135.0, 180.0];
        for (i, &idx) in qc_indices.iter().enumerate() {
            assert!(
                (row[idx] - expected_qc[i]).abs() < 1e-9,
                "qc[{idx}] = {}, expected {}",
                row[idx],
                expected_qc[i]
            );
        }
        // target values are untouched by the c factor
        assert_eq!(row[4], 150.0);
        assert_eq!(row[5], 200.0);
        assert_eq!(row[6], 250.0);
    }

    #[test]
    fn apply_c_factor_leaves_qc_unchanged_when_c_is_non_positive() {
        let raw = vec![10.0, 20.0, 30.0, 40.0, 100.0, 200.0, 300.0];
        let qc_indices = vec![0, 1, 2, 3];
        let target_indices = vec![4, 5, 6];
        // sample spread (sd 200) large enough to push c negative: (200 + (-175/100)*200)/25 = -6
        let mut row = vec![10.0, 20.0, 30.0, 40.0, 0.0, 200.0, 400.0];
        let before = row.clone();

        apply_c_factor(&mut row, &qc_indices, &target_indices, &raw);

        assert_eq!(row, before, "c <= 0 must fall back to a no-op factor of 1");
    }

    #[test]
    fn apply_c_factor_leaves_qc_unchanged_when_c_is_non_finite() {
        // raw sample values are constant (sd = 0), so the B/C term divides by zero.
        let raw = vec![200.0, 300.0, 400.0, 200.0, 200.0, 200.0];
        let qc_indices = vec![0, 1, 2];
        let target_indices = vec![3, 4, 5];
        let mut row = vec![200.0, 300.0, 400.0, 150.0, 200.0, 250.0];
        let before = row.clone();

        apply_c_factor(&mut row, &qc_indices, &target_indices, &raw);

        assert_eq!(row, before, "a non-finite c must fall back to a no-op factor of 1, never poison the row");
    }

    #[test]
    fn c_factor_is_a_no_op_when_the_rf_branch_is_skipped_entirely() {
        // num_vars: 0 forces `select_variables` to always return an empty selection, so every
        // batch takes the raw-passthrough branch and the final rescale is a no-op (normalized
        // already equals raw). In that identity case the c factor must compute to exactly 1 and
        // leave the QC values as the untouched raw values, proving it's wired in without
        // corrupting the already-working pass-through path.
        let train = ndarray::arr2(&[[10.0, 20.0, 30.0, 40.0]]);
        let target = ndarray::arr2(&[[100.0, 200.0, 300.0]]);
        let train_batch = vec!["A".to_string(); 4];
        let target_batch = vec!["A".to_string(); 3];
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 0,
        };

        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.row(0).to_vec(), vec![10.0, 20.0, 30.0, 40.0]);
        assert_eq!(output.normed_target.row(0).to_vec(), vec![100.0, 200.0, 300.0]);
    }

    // Per-batch median rescale (app.R lines 594-597) and outlier-swap (app.R lines 604-617)
    // regression tests. Comparing real SERRF datasets against the R reference showed the port's
    // single whole-group rescale (at the end of compute_compound_row) isn't equivalent to R's
    // per-batch rescale done inside the batch loop — each batch's own median independently gets
    // forced to match the overall raw median in R, not just the combined group's median. The
    // outlier-swap is a safety net R applies afterward, per batch, to catch RF predictions that
    // produced an unstable/extreme correction; it was previously excluded from the port entirely.

    #[test]
    fn rescale_batch_to_overall_median_scales_batch_subset_to_match_target() {
        let mut row = vec![10.0, 20.0, 30.0, 100.0, 200.0, 300.0];
        let batch_indices = vec![0, 1, 2];
        rescale_batch_to_overall_median(&mut row, &batch_indices, 50.0);
        // batch median of [10,20,30] is 20; factor = 50/20 = 2.5
        assert_eq!(row, vec![25.0, 50.0, 75.0, 100.0, 200.0, 300.0]);
    }

    #[test]
    fn rescale_batch_to_overall_median_is_a_no_op_when_batch_median_is_near_zero() {
        let mut row = vec![-10.0, 0.0, 10.0, 999.0];
        let batch_indices = vec![0, 1, 2];
        rescale_batch_to_overall_median(&mut row, &batch_indices, 50.0);
        assert_eq!(row, vec![-10.0, 0.0, 10.0, 999.0]);
    }

    #[test]
    fn outlier_swap_replaces_a_high_outlier_that_moves_toward_center() {
        // Whole-batch normalized values: qc=[10,10,10], target=[10,10,10,50]. n=7 fivenum gives
        // hinges [10,10] (IQR 0), so only the 50 is an outlier (fence is exactly [10,10]).
        let mut row = vec![10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 50.0];
        let batch_cols = vec![0, 1, 2, 3, 4, 5, 6];
        let target_cols = vec![3, 4, 5, 6];
        let raw = vec![0.0, 0.0, 0.0, 12.0, 13.0, 11.0, 90.0];
        let predictions = vec![1.0, 2.0, 3.0, 40.0]; // one per target_cols entry
        let target_mean = 31.5; // mean of raw target values in this batch: (12+13+11+90)/4
        let overall_target_median = 15.0;

        outlier_swap(&mut row, &batch_cols, &target_cols, &raw, &predictions, target_mean, overall_target_median);

        // attempt = raw[6] - (predictions[3] + target_mean - overall_target_median)
        //         = 90 - (40 + 31.5 - 15) = 33.5, and 33.5 < out_mean (50) while out_mean (50) >
        // batch_mean (110/7 ≈ 15.7), so the swap condition holds.
        assert!((row[6] - 33.5).abs() < 1e-9, "row[6] = {}", row[6]);
        // untouched positions
        assert_eq!(&row[0..6], &[10.0, 10.0, 10.0, 10.0, 10.0, 10.0]);
    }

    #[test]
    fn outlier_swap_does_not_replace_when_attempt_does_not_move_toward_center() {
        let mut row = vec![10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 50.0];
        let batch_cols = vec![0, 1, 2, 3, 4, 5, 6];
        let target_cols = vec![3, 4, 5, 6];
        let raw = vec![0.0, 0.0, 0.0, 12.0, 13.0, 11.0, 90.0];
        // predictions/target_mean/overall_target_median chosen so attempt (100) is *not* less
        // than out_mean (50), the required direction when out_mean > batch_mean.
        let predictions = vec![1.0, 2.0, 3.0, -10.0];
        let target_mean = 31.5;
        let overall_target_median = 15.0;

        outlier_swap(&mut row, &batch_cols, &target_cols, &raw, &predictions, target_mean, overall_target_median);

        assert_eq!(row[6], 50.0, "swap must not apply when attempt doesn't reduce the outlier effect");
    }

    #[test]
    fn outlier_swap_is_a_no_op_when_there_are_no_outliers() {
        let mut row = vec![10.0, 11.0, 9.0, 10.0, 11.0, 9.0];
        let before = row.clone();
        let batch_cols = vec![0, 1, 2, 3, 4, 5];
        let target_cols = vec![3, 4, 5];
        let raw = vec![0.0, 0.0, 0.0, 10.0, 11.0, 9.0];
        let predictions = vec![1.0, 2.0, 3.0];

        outlier_swap(&mut row, &batch_cols, &target_cols, &raw, &predictions, 10.0, 10.0);

        assert_eq!(row, before);
    }

    #[test]
    fn scattered_partial_zeros_still_normalize_without_panicking() {
        // Regression check: a few isolated non-detections (not an entire batch) must remain
        // finite and, as before this fix wave, still not worsen QC RSD.
        let (mut train, target, train_batch, target_batch) = synthetic_group();
        train[[0, 0]] = 0.0;
        train[[0, 7]] = 0.0;
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
        assert!(output.normed_train.iter().all(|v| v.is_finite()));
        assert!(output.normed_target.iter().all(|v| v.is_finite()));
    }
}
