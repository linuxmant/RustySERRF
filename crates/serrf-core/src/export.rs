use crate::error::SerrfError;
use std::collections::HashMap;
use std::io::Write;

pub fn std_dev(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() as f64 - 1.0)).sqrt()
}

pub fn filter_rows_with_variance(matrix: &ndarray::Array2<f64>, sds: &[f64]) -> ndarray::Array2<f64> {
    let keep: Vec<usize> = (0..sds.len()).filter(|&i| sds[i] > 0.0).collect();
    matrix.select(ndarray::Axis(0), &keep)
}

/// Drops blank/`None`-sampleType sample columns before PCA, mirroring app.R:1085-1086's
/// `comb_p_pca = comb_p[!is.na(comb_p$sampleType), ]` / `comb_e_pca = comb_e[,!is.na(...)]`.
/// Returns the filtered matrix alongside the sample types for the columns that were kept, in the
/// same order, so the result can be zipped with PCA scores 1:1 when coloring points.
pub fn select_non_blank_columns(matrix: &ndarray::Array2<f64>, sample_type: &[Option<String>]) -> (ndarray::Array2<f64>, Vec<Option<String>>) {
    let keep: Vec<usize> = (0..sample_type.len()).filter(|&i| sample_type[i].is_some()).collect();
    let filtered_matrix = matrix.select(ndarray::Axis(1), &keep);
    let filtered_types = keep.iter().map(|&i| sample_type[i].clone()).collect();
    (filtered_matrix, filtered_types)
}

pub fn write_matrix_csv<W: Write>(writer: W, sample_labels: &[String], compound_labels: &[String], matrix: &ndarray::Array2<f64>) -> Result<(), SerrfError> {
    let mut writer = csv::Writer::from_writer(writer);
    writer.write_record(std::iter::once("label".to_string()).chain(sample_labels.iter().cloned()))?;
    for (i, label) in compound_labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        row.extend(matrix.row(i).iter().map(|v| v.to_string()));
        writer.write_record(&row)?;
    }
    writer.flush().map_err(SerrfError::Io)?;
    Ok(())
}

pub fn write_rsd_csv<W: Write>(
    writer: W,
    labels: &[String],
    raw: &[f64],
    serrf: &[f64],
    validate_rsd_raw: &HashMap<String, Vec<f64>>,
    validate_rsd_serrf: &HashMap<String, Vec<f64>>,
) -> Result<(), SerrfError> {
    let mut writer = csv::Writer::from_writer(writer);
    let mut validate_types: Vec<&String> = validate_rsd_raw.keys().collect();
    validate_types.sort();

    let mut header = vec!["label".to_string(), "QC_none".to_string(), "QC_SERRF".to_string()];
    for t in &validate_types {
        header.push(format!("{t}_none"));
        header.push(format!("{t}_SERRF"));
    }
    writer.write_record(&header)?;

    for (i, label) in labels.iter().enumerate() {
        let mut row = vec![label.clone(), raw[i].to_string(), serrf[i].to_string()];
        for t in &validate_types {
            row.push(validate_rsd_raw[*t][i].to_string());
            row.push(validate_rsd_serrf[*t][i].to_string());
        }
        writer.write_record(&row)?;
    }
    writer.flush().map_err(SerrfError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn std_dev_matches_the_hand_computed_sample_standard_deviation() {
        let result = std_dev(&[1.0, 2.0, 3.0]);
        assert!((result - 1.0).abs() < 1e-12, "expected 1.0, got {result}");
    }

    #[test]
    fn std_dev_of_a_constant_series_is_zero() {
        assert_eq!(std_dev(&[5.0, 5.0, 5.0, 5.0]), 0.0);
    }

    #[test]
    fn filter_rows_with_variance_drops_rows_with_zero_sd() {
        let matrix = array![[1.0, 2.0], [3.0, 3.0], [4.0, 6.0]];
        let sds = [1.0, 0.0, 2.5];
        let filtered = filter_rows_with_variance(&matrix, &sds);
        assert_eq!(filtered.shape(), &[2, 2]);
        assert_eq!(filtered.row(0).to_vec(), vec![1.0, 2.0]);
        assert_eq!(filtered.row(1).to_vec(), vec![4.0, 6.0]);
    }

    #[test]
    fn filter_rows_with_variance_keeps_everything_when_all_sds_are_positive() {
        let matrix = array![[1.0, 2.0], [3.0, 4.0]];
        let sds = [0.5, 0.7];
        let filtered = filter_rows_with_variance(&matrix, &sds);
        assert_eq!(filtered.shape(), matrix.shape());
    }

    #[test]
    fn select_non_blank_columns_drops_none_sample_type_columns() {
        let matrix = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let sample_type = vec![Some("qc".to_string()), None, Some("sample".to_string())];
        let (filtered, kept_types) = select_non_blank_columns(&matrix, &sample_type);
        assert_eq!(filtered.shape(), &[2, 2]);
        assert_eq!(filtered.column(0).to_vec(), vec![1.0, 4.0]);
        assert_eq!(filtered.column(1).to_vec(), vec![3.0, 6.0]);
        assert_eq!(kept_types, vec![Some("qc".to_string()), Some("sample".to_string())]);
    }

    #[test]
    fn select_non_blank_columns_keeps_everything_when_no_column_is_blank() {
        let matrix = array![[1.0, 2.0], [3.0, 4.0]];
        let sample_type = vec![Some("qc".to_string()), Some("sample".to_string())];
        let (filtered, kept_types) = select_non_blank_columns(&matrix, &sample_type);
        assert_eq!(filtered, matrix);
        assert_eq!(kept_types, sample_type);
    }

    #[test]
    fn write_matrix_csv_writes_a_header_using_the_real_sample_labels() {
        let matrix = array![[1.5, 2.5], [3.5, 4.5]];
        let sample_labels = vec!["QC001".to_string(), "GB00042".to_string()];
        let compound_labels = vec!["c1".to_string(), "c2".to_string()];
        let mut buf = Vec::new();
        write_matrix_csv(&mut buf, &sample_labels, &compound_labels, &matrix).unwrap();

        let content = String::from_utf8(buf).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC001,GB00042");
        assert_eq!(lines.next().unwrap(), "c1,1.5,2.5");
        assert_eq!(lines.next().unwrap(), "c2,3.5,4.5");
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_rsd_csv_writes_a_header_and_one_row_per_label() {
        let labels = vec!["c1".to_string(), "c2".to_string()];
        let mut buf = Vec::new();
        write_rsd_csv(&mut buf, &labels, &[0.1, 0.2], &[0.01, 0.02], &HashMap::new(), &HashMap::new()).unwrap();

        let content = String::from_utf8(buf).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC_none,QC_SERRF");
        assert_eq!(lines.next().unwrap(), "c1,0.1,0.01");
        assert_eq!(lines.next().unwrap(), "c2,0.2,0.02");
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_rsd_csv_adds_validate_columns_when_present() {
        let labels = vec!["c1".to_string(), "c2".to_string()];
        let mut validate_raw = HashMap::new();
        validate_raw.insert("validate".to_string(), vec![0.3, 0.4]);
        let mut validate_serrf = HashMap::new();
        validate_serrf.insert("validate".to_string(), vec![0.03, 0.04]);
        let mut buf = Vec::new();
        write_rsd_csv(&mut buf, &labels, &[0.1, 0.2], &[0.01, 0.02], &validate_raw, &validate_serrf).unwrap();

        let content = String::from_utf8(buf).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC_none,QC_SERRF,validate_none,validate_SERRF");
        assert_eq!(lines.next().unwrap(), "c1,0.1,0.01,0.3,0.03");
        assert_eq!(lines.next().unwrap(), "c2,0.2,0.02,0.4,0.04");
        assert!(lines.next().is_none());
    }
}
