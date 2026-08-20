use crate::dataset::{Dataset, RawCompoundTable, RawSampleTable};
use crate::error::SerrfError;
use ndarray::Array2;
use std::collections::HashMap;

pub(crate) fn grid_to_dataset(grid: &[Vec<Option<String>>]) -> Result<Dataset, SerrfError> {
    let nrows = grid.len();
    let ncols = grid.first().map(|r| r.len()).unwrap_or(0);
    if nrows == 0 || ncols == 0 {
        return Err(SerrfError::Parse("input file is empty".into()));
    }

    let sample_col_start = grid[0]
        .iter()
        .position(|c| c.is_some())
        .ok_or_else(|| SerrfError::Parse("the first row of the file is entirely empty".into()))?;
    let compound_row_start = (0..nrows)
        .find(|&r| grid[r][0].is_some())
        .ok_or_else(|| SerrfError::Parse("the first column of the file is entirely empty".into()))?;

    // --- sample metadata: vertical field names live in the corner column ---
    let vertical_field_names: Vec<String> = (0..=compound_row_start)
        .map(|r| grid[r][sample_col_start].clone().unwrap_or_default())
        .collect();
    let field_names_p = rotate_last_to_front(&vertical_field_names);
    if field_names_p.first().map(String::as_str) != Some("label") {
        return Err(SerrfError::Parse(
            "cannot find 'label' in your data. Please check the data format requirement.".into(),
        ));
    }

    let mut sample_label = Vec::new();
    let mut sample_columns: HashMap<String, Vec<String>> = HashMap::new();
    for name in field_names_p.iter().skip(1) {
        sample_columns.entry(name.clone()).or_default();
    }
    for col in (sample_col_start + 1)..ncols {
        let raw: Vec<String> = (0..=compound_row_start)
            .map(|r| grid[r][col].clone().unwrap_or_default())
            .collect();
        let ordered = rotate_last_to_front(&raw);
        sample_label.push(if ordered[0].is_empty() { "na".to_string() } else { ordered[0].clone() });
        for (name, value) in field_names_p.iter().skip(1).zip(ordered.iter().skip(1)) {
            sample_columns.get_mut(name).unwrap().push(value.clone());
        }
    }

    // --- compound metadata: horizontal field names live in the shared header row ---
    let horizontal_field_names: Vec<String> = (0..=sample_col_start)
        .map(|c| grid[compound_row_start][c].clone().unwrap_or_default())
        .collect();
    let field_names_f = rotate_last_to_front(&horizontal_field_names);

    let mut compound_label = Vec::new();
    let mut compound_columns: HashMap<String, Vec<String>> = HashMap::new();
    for name in field_names_f.iter().skip(1) {
        compound_columns.entry(name.clone()).or_default();
    }
    for row in (compound_row_start + 1)..nrows {
        let raw: Vec<String> = (0..=sample_col_start)
            .map(|c| grid[row][c].clone().unwrap_or_default())
            .collect();
        let ordered = rotate_last_to_front(&raw);
        compound_label.push(if ordered[0].is_empty() { "na".to_string() } else { ordered[0].clone() });
        for (name, value) in field_names_f.iter().skip(1).zip(ordered.iter().skip(1)) {
            compound_columns.get_mut(name).unwrap().push(value.clone());
        }
    }

    // --- values matrix ---
    let n_compounds = compound_label.len();
    let n_samples = sample_label.len();
    let mut values = Array2::<f64>::from_elem((n_compounds, n_samples), f64::NAN);
    for (i, row) in ((compound_row_start + 1)..nrows).enumerate() {
        for (j, col) in ((sample_col_start + 1)..ncols).enumerate() {
            if let Some(raw) = &grid[row][col] {
                if let Ok(v) = raw.parse::<f64>() {
                    values[[i, j]] = v;
                }
            }
        }
    }

    Ok(Dataset {
        samples: RawSampleTable { label: sample_label, columns: sample_columns },
        compounds: RawCompoundTable { label: compound_label, columns: compound_columns },
        values,
    })
}

fn rotate_last_to_front(v: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(v.len());
    out.push(v[v.len() - 1].clone());
    out.extend_from_slice(&v[..v.len() - 1]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(s: &str) -> Option<String> { Some(s.to_string()) }

    /// A minimal 6x4 grid mirroring the real file format:
    /// row0-2: batch/sampleType/time values per sample (col0 = NA, col1 = corner field name)
    /// row3:   the shared header row (col0="No", col1(corner)="label", col2/3 = sample NAMES)
    /// row4-5: compound rows (col0="No" value, col1="label" value, col2/3 = numeric values)
    fn sample_grid() -> Vec<Vec<Option<String>>> {
        vec![
            vec![None, cell("batch"), cell("A"), cell("B")],
            vec![None, cell("sampleType"), cell("qc"), cell("sample")],
            vec![None, cell("time"), cell("1"), cell("2")],
            vec![cell("No"), cell("label"), cell("S1"), cell("S2")],
            vec![cell("1"), cell("Compound1"), cell("10.5"), cell("20.5")],
            vec![cell("2"), cell("Compound2"), cell("15.0"), cell("25.0")],
        ]
    }

    #[test]
    fn extracts_sample_metadata() {
        let dataset = grid_to_dataset(&sample_grid()).unwrap();
        assert_eq!(dataset.samples.label, vec!["S1", "S2"]);
        assert_eq!(dataset.samples.columns["batch"], vec!["A", "B"]);
        assert_eq!(dataset.samples.columns["sampleType"], vec!["qc", "sample"]);
        assert_eq!(dataset.samples.columns["time"], vec!["1", "2"]);
    }

    #[test]
    fn extracts_compound_metadata() {
        let dataset = grid_to_dataset(&sample_grid()).unwrap();
        assert_eq!(dataset.compounds.label, vec!["Compound1", "Compound2"]);
        assert_eq!(dataset.compounds.columns["No"], vec!["1", "2"]);
    }

    #[test]
    fn extracts_values_matrix() {
        let dataset = grid_to_dataset(&sample_grid()).unwrap();
        assert_eq!(dataset.values.shape(), &[2, 2]);
        assert_eq!(dataset.values[[0, 0]], 10.5);
        assert_eq!(dataset.values[[0, 1]], 20.5);
        assert_eq!(dataset.values[[1, 0]], 15.0);
        assert_eq!(dataset.values[[1, 1]], 25.0);
    }

    #[test]
    fn errors_when_label_field_is_missing() {
        let mut grid = sample_grid();
        grid[3][1] = None; // corner cell no longer says "label"
        let err = grid_to_dataset(&grid).unwrap_err();
        assert!(err.to_string().contains("label"));
    }
}
