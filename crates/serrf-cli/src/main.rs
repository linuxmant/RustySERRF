use clap::Parser;
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

    write_matrix_csv(&args.output_dir.join("normalized-raw.csv"), &dataset.compounds.label, &output.raw)?;
    write_matrix_csv(&args.output_dir.join("normalized-serrf.csv"), &dataset.compounds.label, &output.serrf)?;
    write_rsd_csv(&args.output_dir.join("qc-rsds.csv"), &dataset.compounds.label, &output.qc_rsd_raw, &output.qc_rsd_serrf)?;

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

fn write_matrix_csv(path: &std::path::Path, labels: &[String], matrix: &ndarray::Array2<f64>) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(std::iter::once("label".to_string()).chain((0..matrix.ncols()).map(|i| format!("sample{i}"))))?;
    for (i, label) in labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        row.extend(matrix.row(i).iter().map(|v| v.to_string()));
        writer.write_record(&row)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_rsd_csv(path: &std::path::Path, labels: &[String], raw: &[f64], serrf: &[f64]) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(["label", "QC_none", "QC_SERRF"])?;
    for (i, label) in labels.iter().enumerate() {
        writer.write_record([label.clone(), raw[i].to_string(), serrf[i].to_string()])?;
    }
    writer.flush()?;
    Ok(())
}
