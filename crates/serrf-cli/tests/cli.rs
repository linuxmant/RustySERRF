use assert_cmd::Command;
use std::path::Path;

#[test]
fn normalizes_the_bundled_example_dataset() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("serrf-cli")
        .unwrap()
        .arg(Path::new("../../golden/example-dataset.xlsx"))
        .arg("--output-dir")
        .arg(temp.path())
        .assert()
        .success();

    assert!(temp.path().join("normalized-serrf.csv").exists());
    assert!(temp.path().join("qc-rsds.csv").exists());

    let content = std::fs::read_to_string(temp.path().join("normalized-serrf.csv")).unwrap();
    assert_eq!(content.lines().count(), 269); // header + 268 compounds
}
