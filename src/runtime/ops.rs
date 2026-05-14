use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::batch::{SessionBatchOp, SessionBatchParams, SessionBatchResponse};
use crate::{CompareOp, GrmError, Result, RuntimeField, RuntimeValueType};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "request", rename_all = "snake_case")]
pub enum RuntimeRequest {
    Schema(SchemaRequest),
    Node(NodeRequest),
    Edge(EdgeRequest),
    Query(QueryRequest),
    Explain(ExplainRequest),
    Profile(ProfileRequest),
    Batch(BatchRequest),
    Admin(AdminRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum SchemaRequest {
    DefineNode(DefineNodeRequest),
    DefineEdge(DefineEdgeRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum NodeRequest {
    Create(NodeCreateRequest),
    Update(NodeUpdateRequest),
    Delete(NodeDeleteRequest),
    Find(NodeFindRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum EdgeRequest {
    Create(EdgeCreateRequest),
    Update(EdgeUpdateRequest),
    Delete(EdgeDeleteRequest),
    Find(EdgeFindRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
pub enum QueryRequest {
    NodeFind(NodeFindRequest),
    EdgeFind(EdgeFindRequest),
    Traversal(TraversalRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefineNodeRequest {
    pub name: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefineEdgeRequest {
    pub name: String,
    pub from_model: String,
    pub to_model: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: FieldValueType,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldValueType {
    String,
    Int,
    Float,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCreateRequest {
    pub model: String,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUpdateRequest {
    pub model: String,
    pub id: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDeleteRequest {
    pub model: String,
    pub id: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeFindRequest {
    pub model: String,
    #[serde(default)]
    pub predicates: Vec<PropertyPredicate>,
    #[serde(default)]
    pub end_predicates: Vec<PropertyPredicate>,
    #[serde(default)]
    pub edge_predicates: Vec<PropertyPredicate>,
    #[serde(default)]
    pub traversals: Vec<TraversalStepRequest>,
    #[serde(default)]
    pub order: Vec<OrderSpec>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub return_mode: Option<TraversalReturn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeCreateRequest {
    pub model: String,
    pub from: i64,
    pub to: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeUpdateRequest {
    pub model: String,
    pub id: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDeleteRequest {
    pub model: String,
    pub id: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeFindRequest {
    pub model: String,
    #[serde(default)]
    pub predicates: Vec<PropertyPredicate>,
    #[serde(default)]
    pub order: Vec<OrderSpec>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub to: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalRequest {
    pub root: NodeFindRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalStepRequest {
    pub direction: TraversalDirection,
    pub edge_model: Option<String>,
    pub end_model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraversalDirection {
    Out,
    In,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraversalReturn {
    End,
    Root,
    Edge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyPredicate {
    pub field: String,
    #[serde(default)]
    pub op: PredicateOp,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateOp {
    #[default]
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderSpec {
    pub field: String,
    pub direction: OrderDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainRequest {
    pub query: QueryRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRequest {
    pub query: QueryRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    #[serde(default = "default_batch_atomic")]
    pub atomic: bool,
    #[serde(default)]
    pub allow_deletes: bool,
    #[serde(default = "default_batch_response")]
    pub response: SessionBatchResponse,
    pub ops: Vec<SessionBatchOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum AdminRequest {
    Save(SaveRequest),
    Load(LoadRequest),
    Export(ExportRequest),
    Import(ImportRequest),
    SchemaList,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveRequest {
    pub format: DurabilityFormat,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadRequest {
    pub format: DurabilityFormat,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurabilityFormat {
    Json,
    Binary,
}

impl From<PredicateOp> for CompareOp {
    fn from(op: PredicateOp) -> Self {
        match op {
            PredicateOp::Eq => Self::Eq,
            PredicateOp::Ne => Self::Ne,
            PredicateOp::Gt => Self::Gt,
            PredicateOp::Ge => Self::Ge,
            PredicateOp::Lt => Self::Lt,
            PredicateOp::Le => Self::Le,
            PredicateOp::Contains => Self::Contains,
        }
    }
}

impl From<FieldValueType> for RuntimeValueType {
    fn from(value_type: FieldValueType) -> Self {
        match value_type {
            FieldValueType::String => Self::String,
            FieldValueType::Int => Self::Int,
            FieldValueType::Float => Self::Float,
            FieldValueType::Bool => Self::Bool,
        }
    }
}

impl TryFrom<FieldSpec> for RuntimeField {
    type Error = GrmError;

    fn try_from(field: FieldSpec) -> Result<Self> {
        Ok(Self {
            name: field.name,
            value_type: field.value_type.into(),
            required: field.required,
        })
    }
}

impl From<BatchRequest> for SessionBatchParams {
    fn from(request: BatchRequest) -> Self {
        Self {
            atomic: request.atomic,
            allow_deletes: request.allow_deletes,
            response: request.response,
            ops: request.ops,
        }
    }
}

fn default_batch_atomic() -> bool {
    true
}

fn default_batch_response() -> SessionBatchResponse {
    SessionBatchResponse::Summary
}
