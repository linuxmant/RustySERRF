use crate::dataset::Dataset;
use crate::error::SerrfError;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedSamples {
    pub label: Vec<String>,
    pub batch: Vec<String>,
    pub sample_type: Vec<Option<String>>,
    pub time: Vec<f64>,
}

pub fn validate(dataset: &Dataset) -> Result<ValidatedSamples, SerrfError> {
    let sample_type_raw = dataset.samples.columns.get("sampleType").ok_or_else(|| {
        SerrfError::Validation("Your data must have 'sampleType'. Please see example data for more information.".into())
    })?;
    let sample_type: Vec<Option<String>> = sample_type_raw
        .iter()
        .map(|s| if s.trim().is_empty() { None } else { Some(s.clone()) })
        .collect();
    let has_qc = sample_type.iter().any(|t| t.as_deref() == Some("qc"));
    let has_sample = sample_type.iter().any(|t| t.as_deref() == Some("sample"));
    if !has_qc || !has_sample {
        return Err(SerrfError::Validation(
            "The 'sampleType' must contain at least 'qc' and 'sample'. Please see example data for more information.".into(),
        ));
    }

    let time_raw = dataset.samples.columns.get("time").ok_or_else(|| {
        SerrfError::Validation("Your data must have 'time'. Please see example data for more information.".into())
    })?;
    let time: Vec<f64> = time_raw
        .iter()
        .map(|s| s.parse::<f64>().map_err(|_| SerrfError::Validation(format!("'time' value '{s}' is not numeric"))))
        .collect::<Result<_, _>>()?;
    let mut seen = HashSet::new();
    for t in &time {
        if !seen.insert(t.to_bits()) {
            return Err(SerrfError::Validation(
                "Your dataset has duplicated 'time' values. 'time' of each sample should be unique.".into(),
            ));
        }
    }

    let batch = dataset
        .samples
        .columns
        .get("batch")
        .ok_or_else(|| SerrfError::Validation("Your data must have 'batch'. Please see example data for more information.".into()))?
        .clone();

    let mut qc_counts: HashMap<&str, usize> = HashMap::new();
    for (b, t) in batch.iter().zip(sample_type.iter()) {
        if t.as_deref() == Some("qc") {
            *qc_counts.entry(b.as_str()).or_insert(0) += 1;
        }
    }
    if qc_counts.values().any(|&c| c < 6) {
        return Err(SerrfError::Validation(
            "Some batches have a small number of QC that is not enough for training the model. Each batch should have at least 6 QCs.".into(),
        ));
    }

    Ok(ValidatedSamples { label: dataset.samples.label.clone(), batch, sample_type, time })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, RawCompoundTable, RawSampleTable};
    use ndarray::Array2;
    use std::collections::HashMap;

    fn dataset_with_samples(columns: HashMap<String, Vec<String>>, n: usize) -> Dataset {
        Dataset {
            samples: RawSampleTable { label: (0..n).map(|i| format!("s{i}")).collect(), columns },
            compounds: RawCompoundTable { label: vec!["c1".into()], columns: HashMap::new() },
            values: Array2::from_elem((1, n), 1.0),
        }
    }

    fn valid_columns() -> HashMap<String, Vec<String>> {
        let mut cols = HashMap::new();
        cols.insert("sampleType".into(), vec!["qc","qc","qc","qc","qc","qc","sample","sample"].iter().map(|s| s.to_string()).collect());
        cols.insert("time".into(), (1..=8).map(|i| i.to_string()).collect());
        cols.insert("batch".into(), vec!["A"; 8].iter().map(|s| s.to_string()).collect());
        cols
    }

    #[test]
    fn accepts_a_valid_dataset() {
        let dataset = dataset_with_samples(valid_columns(), 8);
        let validated = validate(&dataset).unwrap();
        assert_eq!(validated.batch, vec!["A"; 8]);
        assert_eq!(validated.time, (1..=8).map(|i| i as f64).collect::<Vec<_>>());
    }

    #[test]
    fn rejects_missing_sample_type() {
        let mut cols = valid_columns();
        cols.remove("sampleType");
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("sampleType"));
    }

    #[test]
    fn rejects_sample_type_without_qc_and_sample() {
        let mut cols = valid_columns();
        cols.insert("sampleType".into(), vec!["qc"; 8].iter().map(|s| s.to_string()).collect());
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("qc"));
    }

    #[test]
    fn rejects_missing_time() {
        let mut cols = valid_columns();
        cols.remove("time");
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("time"));
    }

    #[test]
    fn rejects_duplicate_time_values() {
        let mut cols = valid_columns();
        cols.insert("time".into(), vec!["1", "1", "3", "4", "5", "6", "7", "8"].iter().map(|s| s.to_string()).collect());
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("duplicated"));
    }

    #[test]
    fn rejects_missing_batch() {
        let mut cols = valid_columns();
        cols.remove("batch");
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("batch"));
    }

    #[test]
    fn rejects_batches_with_too_few_qc() {
        let mut cols = valid_columns();
        cols.insert(
            "sampleType".into(),
            vec!["qc","qc","sample","sample","sample","sample","sample","sample"].iter().map(|s| s.to_string()).collect(),
        );
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("QC"));
    }
}
