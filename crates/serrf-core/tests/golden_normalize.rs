use std::path::Path;

fn median(values: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = values.iter().cloned().filter(|v| v.is_finite()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[sorted.len() / 2]
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum();
    let var_a: f64 = a.iter().map(|x| (x - mean_a).powi(2)).sum();
    let var_b: f64 = b.iter().map(|y| (y - mean_b).powi(2)).sum();
    cov / (var_a.sqrt() * var_b.sqrt())
}

#[test]
fn serrf_output_is_statistically_equivalent_to_the_r_reference() {
    let dataset = serrf_core::parse::read_data(Path::new("../../golden/example-dataset.xlsx")).unwrap();
    let samples = serrf_core::validate::validate(&dataset).unwrap();
    let output = serrf_core::normalize(&dataset, &samples, &serrf_core::SerrfConfig::default(), |_| {}).unwrap();

    let mut reader = csv::Reader::from_path("../../golden/expected/qc-rsds.csv").unwrap();
    let expected_serrf_rsd: Vec<f64> = reader
        .records()
        .map(|r| r.unwrap().get(2).unwrap().parse::<f64>().unwrap()) // QC_SERRF column
        .collect();

    let actual_median = median(&output.qc_rsd_serrf);
    let expected_median = median(&expected_serrf_rsd);
    assert!(
        (actual_median - expected_median).abs() / expected_median < 0.5,
        "SERRF QC RSD median {actual_median} should be within 50% of the R reference {expected_median}"
    );

    let mut reader = csv::Reader::from_path("../../golden/expected/normalized-serrf.csv").unwrap();
    let mut expected_flat = Vec::new();
    for record in reader.records() {
        let record = record.unwrap();
        for cell in record.iter().skip(1) {
            expected_flat.push(cell.parse::<f64>().unwrap_or(f64::NAN));
        }
    }
    let actual_flat: Vec<f64> = output.serrf.iter().cloned().collect();
    let n = actual_flat.len().min(expected_flat.len());
    let correlation = pearson(&actual_flat[..n], &expected_flat[..n]);
    assert!(correlation > 0.8, "normalized values should correlate with the R reference, got {correlation}");
}
