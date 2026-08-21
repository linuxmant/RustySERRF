use crate::cv::cross_validate_qc;
use crate::dataset::Dataset;
use crate::error::SerrfError;
use crate::preprocess::impute_missing;
use crate::rsd::rsd;
use crate::serrf::{serrf_normalize_group, GroupInput};
use crate::validate::ValidatedSamples;
use ndarray::{Array2, Axis};
use std::collections::HashMap;

pub struct SerrfConfig {
    pub num_vars: usize,
    pub seed: u64,
    pub cv_folds: usize,
}

impl Default for SerrfConfig {
    fn default() -> Self {
        Self { num_vars: 10, seed: 1, cv_folds: 5 }
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
    mut progress: impl FnMut(Progress),
) -> Result<PipelineOutput, SerrfError> {
    let mut values = dataset.values.clone();
    impute_missing(&mut values);

    let qc_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some("qc")).collect();
    let sample_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some("sample")).collect();
    let mut validate_types: Vec<String> = samples
        .sample_type
        .iter()
        .filter_map(|t| t.clone())
        .filter(|t| t != "qc" && t != "sample")
        .collect();
    validate_types.sort();
    validate_types.dedup();

    progress(Progress { stage: "raw RSD".into(), current: 0, total: 1 });
    let qc_rsd_raw: Vec<f64> = (0..values.nrows()).map(|i| rsd(&qc_cols.iter().map(|&c| values[[i, c]]).collect::<Vec<_>>())).collect();

    let qc_matrix = values.select(Axis(1), &qc_cols);
    let sample_matrix = values.select(Axis(1), &sample_cols);
    let qc_batch: Vec<String> = qc_cols.iter().map(|&c| samples.batch[c].clone()).collect();
    let sample_batch: Vec<String> = sample_cols.iter().map(|&c| samples.batch[c].clone()).collect();

    progress(Progress { stage: "SERRF normalization".into(), current: 0, total: values.nrows() });
    let group_output = serrf_normalize_group(
        &GroupInput { train: qc_matrix.view(), target: sample_matrix.view(), train_batch: &qc_batch, target_batch: &sample_batch, num_vars: config.num_vars },
        config.seed,
        |current, total| progress(Progress { stage: "SERRF normalization".into(), current, total }),
    );

    progress(Progress { stage: "cross-validation".into(), current: 0, total: 1 });
    let qc_rsd_serrf = cross_validate_qc(&qc_matrix, &qc_batch, config.cv_folds, config.seed, config.num_vars);

    let mut validate_rsd_raw = HashMap::new();
    let mut validate_rsd_serrf = HashMap::new();
    let mut serrf = Array2::<f64>::zeros(values.raw_dim());
    for (idx, &c) in qc_cols.iter().enumerate() {
        for i in 0..values.nrows() {
            serrf[[i, c]] = group_output.normed_train[[i, idx]];
        }
    }
    for (idx, &c) in sample_cols.iter().enumerate() {
        for i in 0..values.nrows() {
            serrf[[i, c]] = group_output.normed_target[[i, idx]];
        }
    }

    for validate_type in &validate_types {
        let validate_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some(validate_type.as_str())).collect();
        let validate_matrix = values.select(Axis(1), &validate_cols);
        let validate_batch: Vec<String> = validate_cols.iter().map(|&c| samples.batch[c].clone()).collect();
        let raw_rsd: Vec<f64> = (0..values.nrows()).map(|i| rsd(&validate_cols.iter().map(|&c| values[[i, c]]).collect::<Vec<_>>())).collect();
        let group = serrf_normalize_group(
            &GroupInput { train: qc_matrix.view(), target: validate_matrix.view(), train_batch: &qc_batch, target_batch: &validate_batch, num_vars: config.num_vars },
            config.seed,
            |_, _| {},
        );
        let normed_rsd: Vec<f64> = (0..values.nrows()).map(|i| rsd(&group.normed_target.row(i).to_vec())).collect();
        for (idx, &c) in validate_cols.iter().enumerate() {
            for i in 0..values.nrows() {
                serrf[[i, c]] = group.normed_target[[i, idx]];
            }
        }
        validate_rsd_raw.insert(validate_type.clone(), raw_rsd);
        validate_rsd_serrf.insert(validate_type.clone(), normed_rsd);
    }

    // columns with no sampleType are passed through unnormalized
    for c in 0..values.ncols() {
        if samples.sample_type[c].is_none() {
            for i in 0..values.nrows() {
                serrf[[i, c]] = values[[i, c]];
            }
        }
    }

    Ok(PipelineOutput { raw: values, serrf, qc_rsd_raw, qc_rsd_serrf, validate_rsd_raw, validate_rsd_serrf, sample_order: samples.label.clone() })
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
            samples: RawSampleTable { label: (0..n).map(|i| format!("s{i}")).collect(), columns: HashMap::new() },
            compounds: RawCompoundTable { label: (0..n_compounds).map(|i| format!("c{i}")).collect(), columns: HashMap::new() },
            values,
        };
        let samples = ValidatedSamples { label: dataset.samples.label.clone(), batch, sample_type, time };
        (dataset, samples)
    }

    #[test]
    fn produces_correctly_shaped_output_and_improves_qc_rsd() {
        let (dataset, samples) = synthetic_dataset();
        let config = SerrfConfig { num_vars: 3, seed: 1, cv_folds: 3 };
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
}
