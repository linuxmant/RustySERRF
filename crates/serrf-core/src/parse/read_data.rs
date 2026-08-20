use crate::dataset::Dataset;
use crate::error::SerrfError;
use crate::parse::{grid_to_dataset, read_csv_grid};
use std::path::Path;

pub fn read_data(path: &Path) -> Result<Dataset, SerrfError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("csv") => grid_to_dataset(&read_csv_grid(path)?),
        Some("xlsx") => grid_to_dataset(&crate::parse::read_xlsx_grid(path, 0)?),
        other => Err(SerrfError::Parse(format!("unsupported file extension: {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_a_full_csv_file_end_to_end() {
        let mut file = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(file, ",batch,A,B").unwrap();
        writeln!(file, ",sampleType,qc,sample").unwrap();
        writeln!(file, ",time,1,2").unwrap();
        writeln!(file, "No,label,S1,S2").unwrap();
        writeln!(file, "1,Compound1,10.5,20.5").unwrap();
        writeln!(file, "2,Compound2,15.0,25.0").unwrap();
        let dataset = read_data(file.path()).unwrap();
        assert_eq!(dataset.samples.label, vec!["S1", "S2"]);
        assert_eq!(dataset.compounds.label, vec!["Compound1", "Compound2"]);
    }

    #[test]
    fn rejects_unsupported_extensions() {
        let file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        assert!(read_data(file.path()).is_err());
    }
}
