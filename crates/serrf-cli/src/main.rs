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

    println!("Done. Output written to {}", args.output_dir.display());
    Ok(())
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
