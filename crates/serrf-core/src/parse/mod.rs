mod grid;
mod layout;
mod read_data;
pub use grid::read_csv_grid;
pub use read_data::read_data;
pub(crate) use layout::grid_to_dataset;

pub(crate) fn read_xlsx_grid(_path: &std::path::Path, _sheet: usize) -> Result<Vec<Vec<Option<String>>>, crate::error::SerrfError> {
    Err(crate::error::SerrfError::Xlsx("not yet implemented".into()))
}
