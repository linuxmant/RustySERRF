mod grid;
mod layout;
mod read_data;
pub use grid::read_csv_grid;
pub(crate) use layout::grid_to_dataset;
pub use read_data::read_data;

pub(crate) fn read_xlsx_grid(path: &std::path::Path, sheet: usize) -> Result<Vec<Vec<Option<String>>>, crate::error::SerrfError> {
    use calamine::{open_workbook, Data, Reader, Xlsx, XlsxError};
    let mut workbook: Xlsx<std::io::BufReader<std::fs::File>> = open_workbook(path).map_err(|e: XlsxError| crate::error::SerrfError::Xlsx(e.to_string()))?;
    let range = workbook
        .worksheet_range_at(sheet)
        .ok_or_else(|| crate::error::SerrfError::Xlsx(format!("sheet index {sheet} out of range")))?
        .map_err(|e| crate::error::SerrfError::Xlsx(e.to_string()))?;

    let grid = range
        .rows()
        .map(|row| {
            row.iter()
                .map(|cell| match cell {
                    Data::Empty => None,
                    Data::String(s) if s.trim().is_empty() => None,
                    Data::String(s) => Some(s.trim().to_string()),
                    Data::Float(f) => Some(f.to_string()),
                    Data::Int(i) => Some(i.to_string()),
                    other => Some(other.to_string()),
                })
                .collect()
        })
        .collect();
    Ok(grid)
}
