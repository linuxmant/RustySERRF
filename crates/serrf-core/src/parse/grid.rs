use crate::error::SerrfError;
use std::path::Path;

pub fn read_csv_grid(path: &Path) -> Result<Vec<Vec<Option<String>>>, SerrfError> {
    let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_path(path)?;
    let mut grid = Vec::new();
    for record in reader.records() {
        let record = record?;
        grid.push(
            record
                .iter()
                .map(|cell| {
                    let trimmed = cell.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .collect(),
        );
    }
    Ok(grid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_a_csv_grid_treating_blanks_as_none() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "a,,c").unwrap();
        writeln!(file, "1,2,").unwrap();
        let grid = read_csv_grid(file.path()).unwrap();
        assert_eq!(
            grid,
            vec![vec![Some("a".into()), None, Some("c".into())], vec![Some("1".into()), Some("2".into()), None],]
        );
    }
}
