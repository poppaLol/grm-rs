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
    AccessPath, BackendCapabilities, BackendIdType, BackendIdentity, CypherQuery, ExecutionPlan,
    GraphBackend, GraphPersistence, GraphTx, InMemoryBackend, IndexEntity, IndexKind,
    IndexMetadata, PlanStep, PlanStepKind, StoredNode, StoredRel, graph_query_to_cypher,
    system_index_catalog,
};
#[cfg(feature = "neo4j")]
pub use backend::{Neo4jBackend, Neo4jConfig, Neo4jTx};
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
    AdminRequest, BatchRequest, CliSession, DefineEdgeRequest, DefineNodeRequest, DurabilityFormat,
    DurableOperation, EdgeCreateRequest, EdgeDeleteRequest, EdgeFindRequest, EdgeRequest,
    EdgeResponse, EdgeUpdateRequest, ExplainRequest, ExportRequest, FieldSpec, FieldValueType,
    ImportRequest, LoadRequest, NodeCreateRequest, NodeDeleteRequest, NodeFindRequest, NodeRequest,
    NodeResponse, NodeUpdateRequest, OrderDirection, OrderSpec, PredicateOp, ProfileRequest,
    PropertyPredicate, QueryRequest, QueryTerm, RuntimeBatchResponse, RuntimeDelete,
    RuntimeDispatchOutcome, RuntimeEdgeDeleteOutcome, RuntimeEdgeFindResponse, RuntimeEdgeOutcome,
    RuntimeField, RuntimeNodeDeleteOutcome, RuntimeNodeFindResponse, RuntimeNodeFindResultResponse,
    RuntimeNodeModel, RuntimeNodeOutcome, RuntimeOperationOutcome, RuntimeRelModel, RuntimeRequest,
    RuntimeResponse, RuntimeSchemaListResponse, RuntimeSchemaOrigin, RuntimeValueType, SaveRequest,
    SchemaRequest, SchemaResponse, SessionBatchDefineEdgeParams, SessionBatchDefineNodeParams,
    SessionBatchEdgeCreateParams, SessionBatchEdgeDeleteParams, SessionBatchEdgeUpdateParams,
    SessionBatchEndpoint, SessionBatchFieldParam, SessionBatchNodeCreateParams,
    SessionBatchNodeDeleteParams, SessionBatchNodeUpdateParams, SessionBatchOp,
    SessionBatchOutcome, SessionBatchParams, SessionBatchResponse, SessionCompactSummary,
    SessionFindResult, SessionModelCatalog, SessionState, TraversalDirection, TraversalRequest,
    TraversalReturn, TraversalStepRequest, Workspace, apply_session_batch,
};

#[cfg(feature = "neo4j")]
pub async fn connect_neo4j_backend(config: Neo4jConfig) -> Result<Neo4jBackend> {
    Neo4jBackend::connect(config).await
}
