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

    // PCA excludes blank/None-sampleType columns entirely (app.R:1085-1086), not just the
    // zero-variance-row filter below.
    let (raw_non_blank, pca_sample_type) = serrf_core::export::select_non_blank_columns(&output.raw, &samples.sample_type);
    let sds_before: Vec<f64> = (0..raw_non_blank.nrows())
        .map(|i| serrf_core::export::std_dev(&raw_non_blank.row(i).to_vec()))
        .collect();
    let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&raw_non_blank, &sds_before));

    let (serrf_non_blank, _) = serrf_core::export::select_non_blank_columns(&output.serrf, &samples.sample_type);
    let sds_after: Vec<f64> = (0..serrf_non_blank.nrows())
        .map(|i| serrf_core::export::std_dev(&serrf_non_blank.row(i).to_vec()))
        .collect();
    let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&serrf_non_blank, &sds_after));

    serrf_core::report::render_report(
        &args.output_dir.join("report.png"),
        &output.qc_rsd_raw,
        &output.qc_rsd_serrf,
        &output.validate_rsd_raw,
        &output.validate_rsd_serrf,
        &pca_before,
        &pca_after,
        &pca_sample_type,
    )?;

    println!("Done. Output written to {}", args.output_dir.display());
    Ok(())
}

fn write_matrix_csv(path: &std::path::Path, sample_labels: &[String], compound_labels: &[String], matrix: &ndarray::Array2<f64>) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    serrf_core::export::write_matrix_csv(file, sample_labels, compound_labels, matrix)?;
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
    let file = std::fs::File::create(path)?;
    serrf_core::export::write_rsd_csv(file, labels, raw, serrf, validate_rsd_raw, validate_rsd_serrf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn write_matrix_csv_writes_a_header_using_the_real_sample_labels() {
        // I5: the header must use the pipeline's real sample labels (`output.sample_order`),
        // not generic `sample0`/`sample1` placeholders, so the CSV can be joined back to sample
        // metadata (batch/time/type).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matrix.csv");
        let matrix = array![[1.0, 2.0], [3.0, 4.0]];
        let sample_labels = vec!["QC001".to_string(), "GB00042".to_string()];
        let compound_labels = vec!["c1".to_string(), "c2".to_string()];
        write_matrix_csv(&path, &sample_labels, &compound_labels, &matrix).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC001,GB00042");
        assert_eq!(lines.next().unwrap(), "c1,1,2");
        assert_eq!(lines.next().unwrap(), "c2,3,4");
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
