use crate::cv::cross_validate_qc;
use crate::dataset::Dataset;
use crate::error::SerrfError;
use crate::preprocess::{extract_infinite_rows, impute_missing};
use crate::rsd::rsd;
use crate::serrf::{serrf_normalize_group, GroupInput};
use crate::validate::ValidatedSamples;
use ndarray::{Array2, Axis};
use std::collections::{HashMap, HashSet};

pub struct SerrfConfig {
    pub num_vars: usize,
    pub seed: u64,
    pub cv_folds: usize,
}

impl Default for SerrfConfig {
    fn default() -> Self {
        Self {
            num_vars: 10,
            seed: 1,
            cv_folds: 5,
        }
    }
}

pub struct Progress {
    pub stage: String,
    pub current: usize,
    pub total: usize,
}

pub struct PipelineOutput {
    pub raw: Array2<f64>,
    pub serrf: Array2<f64>,
    pub qc_rsd_raw: Vec<f64>,
    pub qc_rsd_serrf: Vec<f64>,
    pub validate_rsd_raw: HashMap<String, Vec<f64>>,
    pub validate_rsd_serrf: HashMap<String, Vec<f64>>,
    pub sample_order: Vec<String>,
}

pub fn normalize(
    dataset: &Dataset,
    samples: &ValidatedSamples,
    config: &SerrfConfig,
    mut progress: impl FnMut(Progress) + Send,
) -> Result<PipelineOutput, SerrfError> {
    let mut values = dataset.values.clone();
    impute_missing(&mut values);

    let n_compounds_total = values.nrows();

    // Strip compound rows that are entirely non-finite across the normalization-relevant
    // columns (blank/`None` sampleType columns are excluded, mirroring app.R:297-309's check on
    // `e` after blank-sampleType columns are dropped) before they ever reach SERRF/RF training.
    // `impute_missing` maps an entirely-missing (`NaN`) row to an entirely-`Inf` row (matching
    // app.R:272's `0.5 * min(numeric(0))` == `Inf`), so `extract_infinite_rows` catches both the
    // "compound row is entirely +Inf" and "compound entirely missing" C1 scenarios here. These
    // rows are re-inserted as NaN rows in the final raw/serrf matrices and per-compound RSD
    // vectors below, matching app.R:801-813/835-843's strip-then-reinsert pattern.
    let non_blank_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].is_some()).collect();
    let infinite_rows: HashSet<usize> = {
        let non_blank_matrix = values.select(Axis(1), &non_blank_cols);
        extract_infinite_rows(&non_blank_matrix).into_iter().collect()
    };
    let finite_row_indices: Vec<usize> = (0..n_compounds_total).filter(|i| !infinite_rows.contains(i)).collect();
    let working = values.select(Axis(0), &finite_row_indices);

    let qc_cols: Vec<usize> = (0..working.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some("qc")).collect();
    let sample_cols: Vec<usize> = (0..working.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some("sample")).collect();
    let mut validate_types: Vec<String> = samples
        .sample_type
        .iter()
        .filter_map(|t| t.clone())
        .filter(|t| t != "qc" && t != "sample")
        .collect();
    validate_types.sort();
    validate_types.dedup();

    progress(Progress {
        stage: "raw RSD".into(),
        current: 0,
        total: 1,
    });
    let qc_rsd_raw_finite: Vec<f64> = (0..working.nrows())
        .map(|i| rsd(&qc_cols.iter().map(|&c| working[[i, c]]).collect::<Vec<_>>()))
        .collect();

    let qc_matrix = working.select(Axis(1), &qc_cols);
    let sample_matrix = working.select(Axis(1), &sample_cols);
    let qc_batch: Vec<String> = qc_cols.iter().map(|&c| samples.batch[c].clone()).collect();
    let sample_batch: Vec<String> = sample_cols.iter().map(|&c| samples.batch[c].clone()).collect();

    progress(Progress {
        stage: "SERRF normalization".into(),
        current: 0,
        total: working.nrows(),
    });
    let group_output = serrf_normalize_group(
        &GroupInput {
            train: qc_matrix.view(),
            target: sample_matrix.view(),
            train_batch: &qc_batch,
            target_batch: &sample_batch,
            num_vars: config.num_vars,
        },
        config.seed,
        |current, total| {
            progress(Progress {
                stage: "SERRF normalization".into(),
                current,
                total,
            })
        },
    );

    progress(Progress {
        stage: "cross-validation".into(),
        current: 0,
        total: 1,
    });
    let qc_rsd_serrf_finite = cross_validate_qc(&qc_matrix, &qc_batch, config.cv_folds, config.seed, config.num_vars);

    let mut validate_rsd_raw_finite = HashMap::new();
    let mut validate_rsd_serrf_finite = HashMap::new();
    let mut serrf_working = Array2::<f64>::zeros(working.raw_dim());
    for (idx, &c) in qc_cols.iter().enumerate() {
        for i in 0..working.nrows() {
            serrf_working[[i, c]] = group_output.normed_train[[i, idx]];
        }
    }
    for (idx, &c) in sample_cols.iter().enumerate() {
        for i in 0..working.nrows() {
            serrf_working[[i, c]] = group_output.normed_target[[i, idx]];
        }
    }

    for validate_type in &validate_types {
        let validate_cols: Vec<usize> = (0..working.ncols())
            .filter(|&c| samples.sample_type[c].as_deref() == Some(validate_type.as_str()))
            .collect();
        let validate_matrix = working.select(Axis(1), &validate_cols);
        let validate_batch: Vec<String> = validate_cols.iter().map(|&c| samples.batch[c].clone()).collect();
        let raw_rsd: Vec<f64> = (0..working.nrows())
            .map(|i| rsd(&validate_cols.iter().map(|&c| working[[i, c]]).collect::<Vec<_>>()))
            .collect();
        let group = serrf_normalize_group(
            &GroupInput {
                train: qc_matrix.view(),
                target: validate_matrix.view(),
                train_batch: &qc_batch,
                target_batch: &validate_batch,
                num_vars: config.num_vars,
            },
            config.seed,
            |_, _| {},
        );
        let normed_rsd: Vec<f64> = (0..working.nrows()).map(|i| rsd(&group.normed_target.row(i).to_vec())).collect();
        for (idx, &c) in validate_cols.iter().enumerate() {
            for i in 0..working.nrows() {
                serrf_working[[i, c]] = group.normed_target[[i, idx]];
            }
        }
        validate_rsd_raw_finite.insert(validate_type.clone(), raw_rsd);
        validate_rsd_serrf_finite.insert(validate_type.clone(), normed_rsd);
    }

    // columns with no sampleType are passed through unnormalized
    for c in 0..working.ncols() {
        if samples.sample_type[c].is_none() {
            for i in 0..working.nrows() {
                serrf_working[[i, c]] = working[[i, c]];
            }
        }
    }

    // Reassemble full-size outputs: rows that were stripped above as entirely non-finite are
    // re-inserted as NaN (raw/serrf matrices and every per-compound RSD vector); every other row
    // gets its computed value. This keeps `raw`/`serrf`'s row count, and every RSD vector's
    // length, equal to the original compound count regardless of how many rows were stripped.
    let n_samples = values.ncols();
    let mut raw = Array2::<f64>::from_elem((n_compounds_total, n_samples), f64::NAN);
    let mut serrf = Array2::<f64>::from_elem((n_compounds_total, n_samples), f64::NAN);
    for (working_i, &orig_i) in finite_row_indices.iter().enumerate() {
        for c in 0..n_samples {
            raw[[orig_i, c]] = working[[working_i, c]];
            serrf[[orig_i, c]] = serrf_working[[working_i, c]];
        }
    }

    let reinsert = |finite_vals: &[f64]| -> Vec<f64> {
        let mut out = vec![f64::NAN; n_compounds_total];
        for (working_i, &orig_i) in finite_row_indices.iter().enumerate() {
            out[orig_i] = finite_vals[working_i];
        }
        out
    };
    let qc_rsd_raw = reinsert(&qc_rsd_raw_finite);
    let qc_rsd_serrf = reinsert(&qc_rsd_serrf_finite);
    let validate_rsd_raw: HashMap<String, Vec<f64>> = validate_rsd_raw_finite.into_iter().map(|(k, v)| (k, reinsert(&v))).collect();
    let validate_rsd_serrf: HashMap<String, Vec<f64>> = validate_rsd_serrf_finite.into_iter().map(|(k, v)| (k, reinsert(&v))).collect();

    Ok(PipelineOutput {
        raw,
        serrf,
        qc_rsd_raw,
        qc_rsd_serrf,
        validate_rsd_raw,
        validate_rsd_serrf,
        sample_order: samples.label.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, RawCompoundTable, RawSampleTable};
    use crate::validate::ValidatedSamples;
    use ndarray::Array2;
    use std::collections::HashMap;

    fn synthetic_dataset() -> (Dataset, ValidatedSamples) {
        let n_compounds = 5;
        let n_qc = 12;
        let n_sample = 8;
        let n = n_qc + n_sample;
        let mut values = Array2::<f64>::zeros((n_compounds, n));
        let mut batch = Vec::new();
        let mut sample_type = Vec::new();
        let mut time = Vec::new();
        for j in 0..n {
            let is_qc = j < n_qc;
            let b = if j % 4 < 2 { "A" } else { "B" };
            let drift = if b == "A" { 0.0 } else { 8.0 };
            batch.push(b.to_string());
            sample_type.push(Some(if is_qc { "qc" } else { "sample" }.to_string()));
            time.push(j as f64);
            for i in 0..n_compounds {
                values[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        let dataset = Dataset {
            samples: RawSampleTable {
                label: (0..n).map(|i| format!("s{i}")).collect(),
                columns: HashMap::new(),
            },
            compounds: RawCompoundTable {
                label: (0..n_compounds).map(|i| format!("c{i}")).collect(),
                columns: HashMap::new(),
            },
            values,
        };
        let samples = ValidatedSamples {
            label: dataset.samples.label.clone(),
            batch,
            sample_type,
            time,
        };
        (dataset, samples)
    }

    #[test]
    fn produces_correctly_shaped_output_and_improves_qc_rsd() {
        let (dataset, samples) = synthetic_dataset();
        let config = SerrfConfig {
            num_vars: 3,
            seed: 1,
            cv_folds: 3,
        };
        let output = normalize(&dataset, &samples, &config, |_| {}).unwrap();

        assert_eq!(output.raw.shape(), dataset.values.shape());
        assert_eq!(output.serrf.shape(), dataset.values.shape());
        assert_eq!(output.qc_rsd_raw.len(), 5);
        assert_eq!(output.qc_rsd_serrf.len(), 5);

        let median = |v: &[f64]| {
            let mut sorted = v.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted[sorted.len() / 2]
        };
        assert!(median(&output.qc_rsd_serrf) <= median(&output.qc_rsd_raw));
    }

    /// C1 regression test: a compound row that is entirely `+Inf` (app.R:307-316's
    /// strip-then-reinsert scenario) must not panic `normalize()`, and must come back out as a
    /// clearly-marked (`NaN`) row at its original position in both `raw` and `serrf`, while every
    /// other compound is normalized normally.
    #[test]
    fn strips_and_reinserts_an_entirely_infinite_compound_row_as_nan() {
        let (mut dataset, samples) = synthetic_dataset();
        let infinite_row = 2;
        for c in 0..dataset.values.ncols() {
            dataset.values[[infinite_row, c]] = f64::INFINITY;
        }
        let config = SerrfConfig {
            num_vars: 3,
            seed: 1,
            cv_folds: 3,
        };
        let output = normalize(&dataset, &samples, &config, |_| {}).unwrap();

        let n_compounds = dataset.values.nrows();
        assert_eq!(output.raw.shape(), dataset.values.shape());
        assert_eq!(output.serrf.shape(), dataset.values.shape());
        assert_eq!(output.qc_rsd_raw.len(), n_compounds);
        assert_eq!(output.qc_rsd_serrf.len(), n_compounds);

        // The stripped row comes back as NaN everywhere it's reported...
        assert!(
            output.raw.row(infinite_row).iter().all(|v| v.is_nan()),
            "expected the stripped row to be reinserted as NaN in `raw`"
        );
        assert!(
            output.serrf.row(infinite_row).iter().all(|v| v.is_nan()),
            "expected the stripped row to be reinserted as NaN in `serrf`"
        );
        assert!(output.qc_rsd_raw[infinite_row].is_nan());
        assert!(output.qc_rsd_serrf[infinite_row].is_nan());

        // ...while every other compound normalizes as if the bad row wasn't there at all.
        for i in 0..n_compounds {
            if i == infinite_row {
                continue;
            }
            assert!(
                output.raw.row(i).iter().all(|v| v.is_finite()),
                "row {i} should be untouched by the stripped row"
            );
            assert!(output.serrf.row(i).iter().all(|v| v.is_finite()), "row {i} should normalize normally");
            assert!(output.qc_rsd_raw[i].is_finite());
            assert!(output.qc_rsd_serrf[i].is_finite());
        }
    }

    /// Builds a dataset with an extra "validate"-type group (a validate sample type distinct
    /// from "qc"/"sample") and a couple of blank/`None` sampleType columns, so we can exercise
    /// the validate-type branch and the None-sampleType passthrough branch of `normalize()`
    /// directly. `ValidatedSamples` is constructed by hand here (bypassing `validate::validate`),
    /// so the per-batch QC-count constraint that function enforces doesn't apply.
    fn synthetic_dataset_with_validate_and_blank() -> (Dataset, ValidatedSamples) {
        let n_compounds = 5;
        let n_qc = 12;
        let n_sample = 8;
        let n_validate = 8;
        let n_blank = 2;
        let n = n_qc + n_sample + n_validate + n_blank;
        let mut values = Array2::<f64>::zeros((n_compounds, n));
        let mut batch = Vec::new();
        let mut sample_type = Vec::new();
        let mut time = Vec::new();
        for j in 0..n {
            let b = if j % 4 < 2 { "A" } else { "B" };
            let drift = if b == "A" { 0.0 } else { 8.0 };
            batch.push(b.to_string());
            let st = if j < n_qc {
                Some("qc".to_string())
            } else if j < n_qc + n_sample {
                Some("sample".to_string())
            } else if j < n_qc + n_sample + n_validate {
                Some("validate".to_string())
            } else {
                None
            };
            sample_type.push(st);
            time.push(j as f64);
            for i in 0..n_compounds {
                values[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        let dataset = Dataset {
            samples: RawSampleTable {
                label: (0..n).map(|i| format!("s{i}")).collect(),
                columns: HashMap::new(),
            },
            compounds: RawCompoundTable {
                label: (0..n_compounds).map(|i| format!("c{i}")).collect(),
                columns: HashMap::new(),
            },
            values,
        };
        let samples = ValidatedSamples {
            label: dataset.samples.label.clone(),
            batch,
            sample_type,
            time,
        };
        (dataset, samples)
    }

    #[test]
    fn produces_validate_type_rsd_entries_and_passes_through_blank_columns() {
        let (dataset, samples) = synthetic_dataset_with_validate_and_blank();
        let config = SerrfConfig {
            num_vars: 3,
            seed: 1,
            cv_folds: 3,
        };
        let output = normalize(&dataset, &samples, &config, |_| {}).unwrap();

        let n_compounds = dataset.values.nrows();

        // The "validate" sample-type group must produce raw/serrf RSD entries, one per compound.
        assert!(
            output.validate_rsd_raw.contains_key("validate"),
            "expected a 'validate' entry in validate_rsd_raw"
        );
        assert!(
            output.validate_rsd_serrf.contains_key("validate"),
            "expected a 'validate' entry in validate_rsd_serrf"
        );
        assert_eq!(output.validate_rsd_raw["validate"].len(), n_compounds);
        assert_eq!(output.validate_rsd_serrf["validate"].len(), n_compounds);

        // Blank/None sampleType columns must pass through unnormalized: serrf == raw exactly.
        let blank_cols: Vec<usize> = (0..samples.sample_type.len()).filter(|&c| samples.sample_type[c].is_none()).collect();
        assert_eq!(blank_cols.len(), 2, "expected exactly the 2 blank columns from the fixture");
        for &c in &blank_cols {
            for i in 0..n_compounds {
                assert_eq!(
                    output.serrf[[i, c]],
                    output.raw[[i, c]],
                    "blank column {c} row {i} should pass through unnormalized"
                );
            }
        }
    }
}
