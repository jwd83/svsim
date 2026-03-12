use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub message: String,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Unsupported(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
