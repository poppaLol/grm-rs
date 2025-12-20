pub mod backend;
pub mod dsl;
pub mod error;
pub mod macros;
pub mod model;
pub mod repo;
pub mod client;
pub mod decode;

// Re-exports for convenient use
pub use backend::{GraphBackend, GraphTx, InMemoryBackend, StoredNode, StoredRel};
pub use dsl::{
    CompareOp, GraphQuery, NodePattern, Property, PropertyFilter, Props, Query, QueryKind,
    QueryResult, VarGen, QueryRow
};
pub use error::{GrmError, Result};
pub use grm_rs_macros::*;
pub use model::{NodeModel, RelModel};
pub use repo::{NodeRepository, RelRepository};
pub use client::GraphClient;
pub use decode::DecodeFromRow;