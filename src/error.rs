use std::path::PathBuf;
use thiserror::Error;

/// Errors produced by `csv_coincidence`.
#[derive(Debug, Error)]
pub enum CsvCoincidenceError {
    #[error("the file is not a valid CSV file: {0}")]
    InvalidExtension(PathBuf),

    #[error("invalid regular expression: {0}")]
    InvalidRegex(#[from] regex::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("column not found: {0}")]
    ColumnNotFound(String),

    #[error("invalid column index: {0}")]
    InvalidColumnIndex(usize),

    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, CsvCoincidenceError>;
