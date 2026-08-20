#[derive(Debug, thiserror::Error)]
pub enum SerrfError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("xlsx error: {0}")]
    Xlsx(String),
    #[error("could not parse input: {0}")]
    Parse(String),
    #[error("validation failed: {0}")]
    Validation(String),
}
