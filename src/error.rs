use thiserror::Error;

#[derive(Debug, Error)]
pub enum GrmError {
    #[error("backend error: {0}")]
    Backend(String),

    #[error("mapping error: {0}")]
    Mapping(String),

    #[error("not found")]
    NotFound,

    #[error("constraint violation: {0}")]
    Constraint(String),

    #[error("tx already committed/rolled back")]
    TransactionClosed,

    #[error("operation not supported by backend: {0}")]
    NotSupported(&'static str),

    #[error("cannot save to file: {0}")]
    SaveAborted(&'static str),

    #[error("cannot load from file: {0}")]
    LoadAborted(&'static str),
}

pub type Result<T> = std::result::Result<T, GrmError>;
