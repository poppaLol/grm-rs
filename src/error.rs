use thiserror::Error;

#[derive(Debug, Error)]
pub enum GrmError {
    /// Backend-specific failure that does not fit a narrower portable category,
    /// such as an adapter error, driver failure, or unsupported native query
    /// path that predates `NotSupported`.
    #[error("backend error: {0}")]
    Backend(String),

    /// The library could not map between typed/runtime data and kernel rows,
    /// values, schemas, or query shapes.
    #[error("mapping error: {0}")]
    Mapping(String),

    /// Requested entity was not found when the caller expected one.
    #[error("not found")]
    NotFound,

    /// A user-visible invariant or schema/data constraint was violated.
    #[error("constraint violation: {0}")]
    Constraint(String),

    /// A transaction wrapper was used after it had already been consumed.
    #[error("tx already committed/rolled back")]
    TransactionClosed,

    /// The backend or surface intentionally does not implement this operation.
    #[error("operation not supported by backend: {0}")]
    NotSupported(&'static str),

    /// Persistence refused to save because the target or format is invalid.
    #[error("cannot save to file: {0}")]
    SaveAborted(&'static str),

    /// Persistence refused to load because the source or format is invalid.
    #[error("cannot load from file: {0}")]
    LoadAborted(&'static str),
}

pub type Result<T> = std::result::Result<T, GrmError>;
