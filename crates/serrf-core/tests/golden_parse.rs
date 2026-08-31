use std::path::Path;

#[test]
fn parses_the_bundled_example_dataset() {
    let dataset = serrf_core::parse::read_data(Path::new("../../golden/example-dataset.xlsx")).unwrap();
    assert_eq!(dataset.samples.label.len(), 1299);
    assert_eq!(dataset.compounds.label.len(), 268);
    assert_eq!(dataset.values.shape(), &[268, 1299]);

    // cross-check against the reference sample metadata already exported from R
    let mut reader = csv::Reader::from_path("../../golden/expected/comb_p.csv").unwrap();
    let expected_batches: Vec<String> = reader.records().map(|r| r.unwrap().get(1).unwrap().to_string()).collect();
    assert_eq!(dataset.samples.columns["batch"], expected_batches);
}
