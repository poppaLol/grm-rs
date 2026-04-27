pub mod backend;
pub mod client;
pub mod decode;
pub mod dsl;
pub mod error;
mod fsutil;
pub mod macros;
pub mod model;
pub mod repo;
pub mod runtime;

// Re-exports for convenient use
pub use backend::{
    BackendIdType, BackendIdentity, GraphBackend, GraphPersistence, GraphTx, InMemoryBackend,
    StoredNode, StoredRel,
};
pub use client::{GraphClient, GraphPersistenceAccess};
pub use decode::{DecodeFromRow, ResultShape, labels_match, node, rel};
pub use dsl::{
    CompareOp, GraphQuery, KernelValue, NodePattern, Property, PropertyFilter, Props, Query,
    QueryKind, QueryResult, QueryRow, ReturnKind, VarGen,
};
pub use error::{GrmError, Result};
pub use grm_rs_macros::*;
pub use model::{NodeModel, RelModel};
pub use repo::{NodeRepository, RelRepository, Repo};
pub use runtime::{
    CliSession, RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeValueType,
    SessionCompactSummary, SessionModelCatalog, SessionState,
};
