use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    input: PathBuf,
    #[arg(short, long, default_value = "./output")]
    output_dir: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.output_dir)?;

    let dataset = serrf_core::parse::read_data(&args.input)?;
    let samples = serrf_core::validate::validate(&dataset)?;
    let output = serrf_core::normalize(&dataset, &samples, &serrf_core::SerrfConfig::default(), |p| {
        println!("[{}] {}/{}", p.stage, p.current, p.total);
    })?;

    // Named "imputed", not "raw": `output.raw` is `dataset.values` after `impute_missing` has
    // already filled in missing cells (pipeline.rs), so it is not the literal raw input matrix.
    write_matrix_csv(
        &args.output_dir.join("normalized-imputed.csv"),
        &output.sample_order,
        &dataset.compounds.label,
        &output.raw,
    )?;
    write_matrix_csv(
        &args.output_dir.join("normalized-serrf.csv"),
        &output.sample_order,
        &dataset.compounds.label,
        &output.serrf,
    )?;
    write_rsd_csv(
        &args.output_dir.join("qc-rsds.csv"),
        &dataset.compounds.label,
        &output.qc_rsd_raw,
        &output.qc_rsd_serrf,
        &output.validate_rsd_raw,
        &output.validate_rsd_serrf,
    )?;

    let sds_before: Vec<f64> = (0..dataset.values.nrows()).map(|i| std_dev(&output.raw.row(i).to_vec())).collect();
    let pca_before = serrf_core::pca::pca_first_two(&filter_rows_with_variance(&output.raw, &sds_before));
    let sds_after: Vec<f64> = (0..dataset.values.nrows()).map(|i| std_dev(&output.serrf.row(i).to_vec())).collect();
    let pca_after = serrf_core::pca::pca_first_two(&filter_rows_with_variance(&output.serrf, &sds_after));
    serrf_core::report::render_report(
        &args.output_dir.join("report.png"),
        &output.qc_rsd_raw,
        &output.qc_rsd_serrf,
        &pca_before,
        &pca_after,
        &samples.sample_type,
    )?;

    println!("Done. Output written to {}", args.output_dir.display());
    Ok(())
}

fn std_dev(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() as f64 - 1.0)).sqrt()
}

fn filter_rows_with_variance(matrix: &ndarray::Array2<f64>, sds: &[f64]) -> ndarray::Array2<f64> {
    let keep: Vec<usize> = (0..sds.len()).filter(|&i| sds[i] > 0.0).collect();
    matrix.select(ndarray::Axis(0), &keep)
}

fn write_matrix_csv(path: &std::path::Path, sample_labels: &[String], compound_labels: &[String], matrix: &ndarray::Array2<f64>) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    // Use the pipeline's real sample labels (`PipelineOutput::sample_order`) as column headers,
    // not generic `sample0`/`sample1` placeholders (I5), so the CSV can be joined back to sample
    // metadata (batch/time/type).
    writer.write_record(std::iter::once("label".to_string()).chain(sample_labels.iter().cloned()))?;
    for (i, label) in compound_labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        row.extend(matrix.row(i).iter().map(|v| v.to_string()));
        writer.write_record(&row)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_rsd_csv(
    path: &std::path::Path,
    labels: &[String],
    raw: &[f64],
    serrf: &[f64],
    validate_rsd_raw: &HashMap<String, Vec<f64>>,
    validate_rsd_serrf: &HashMap<String, Vec<f64>>,
) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    // Extra `{type}_none`/`{type}_SERRF` columns for each validate-type sampleType the pipeline
    // found (I5), matching R's reference `qc-rsds.csv` format (which has `validate_none`/
    // `validate_SERRF` columns) instead of silently dropping this already-computed RSD data.
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
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn std_dev_matches_the_hand_computed_sample_standard_deviation() {
        // mean = 2.0; sample variance = ((1-2)^2 + (2-2)^2 + (3-2)^2) / (3-1) = 1.0
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
    fn write_matrix_csv_writes_a_header_using_the_real_sample_labels() {
        // I5: the header must use the pipeline's real sample labels (`output.sample_order`),
        // not generic `sample0`/`sample1` placeholders, so the CSV can be joined back to sample
        // metadata (batch/time/type).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matrix.csv");
        let matrix = array![[1.5, 2.5], [3.5, 4.5]];
        let sample_labels = vec!["QC001".to_string(), "GB00042".to_string()];
        let compound_labels = vec!["c1".to_string(), "c2".to_string()];
        write_matrix_csv(&path, &sample_labels, &compound_labels, &matrix).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC001,GB00042");
        assert_eq!(lines.next().unwrap(), "c1,1.5,2.5");
        assert_eq!(lines.next().unwrap(), "c2,3.5,4.5");
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_rsd_csv_writes_a_header_and_one_row_per_label() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rsds.csv");
        let labels = vec!["c1".to_string(), "c2".to_string()];
        write_rsd_csv(&path, &labels, &[0.1, 0.2], &[0.01, 0.02], &HashMap::new(), &HashMap::new()).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC_none,QC_SERRF");
        assert_eq!(lines.next().unwrap(), "c1,0.1,0.01");
        assert_eq!(lines.next().unwrap(), "c2,0.2,0.02");
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_rsd_csv_adds_validate_columns_when_present() {
        // I5: R's reference qc-rsds.csv has `validate_none`/`validate_SERRF` columns; the CLI
        // now writes them too when the pipeline produced validate-type RSDs.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rsds.csv");
        let labels = vec!["c1".to_string(), "c2".to_string()];
        let mut validate_raw = HashMap::new();
        validate_raw.insert("validate".to_string(), vec![0.3, 0.4]);
        let mut validate_serrf = HashMap::new();
        validate_serrf.insert("validate".to_string(), vec![0.03, 0.04]);
        write_rsd_csv(&path, &labels, &[0.1, 0.2], &[0.01, 0.02], &validate_raw, &validate_serrf).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC_none,QC_SERRF,validate_none,validate_SERRF");
        assert_eq!(lines.next().unwrap(), "c1,0.1,0.01,0.3,0.03");
        assert_eq!(lines.next().unwrap(), "c2,0.2,0.02,0.4,0.04");
        assert!(lines.next().is_none());
    }
}
