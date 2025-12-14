pub mod backend;
pub mod model;
pub mod error;
pub mod repo;
pub mod macros;
pub mod dsl;

// Re-exports for convenient use
pub use backend::{GraphBackend, GraphTx, StoredNode, StoredRel, InMemoryBackend};
pub use model::{NodeModel, RelModel};
pub use error::{GrmError, Result};
pub use repo::{NodeRepository, RelRepository};
pub use grm_rs_macros::*;
pub use dsl::{NodePattern, Property, PropertyFilter, CompareOp, GraphQuery, Query, QueryKind, VarGen, QueryResult};