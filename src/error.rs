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
}

pub type Result<T> = std::result::Result<T, GrmError>;
