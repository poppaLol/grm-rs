use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::batch::{SessionBatchOp, SessionBatchParams, SessionBatchResponse};
use crate::{
    CompareOp, DurableOperation, GrmError, Result, RuntimeField, RuntimeNodeModel, RuntimeRelModel,
    RuntimeValueType, StoredNode, StoredRel,
};

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
#[serde(tag = "type", content = "response", rename_all = "snake_case")]
pub enum RuntimeResponse {
    Schema(SchemaResponse),
    Node(NodeResponse),
    Edge(EdgeResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDispatchOutcome {
    pub response: RuntimeResponse,
    #[serde(default)]
    pub durable_ops: Vec<DurableOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum SchemaRequest {
    DefineNode(DefineNodeRequest),
    DefineEdge(DefineEdgeRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "result", rename_all = "snake_case")]
pub enum SchemaResponse {
    DefineNode(RuntimeNodeModel),
    DefineEdge(RuntimeRelModel),
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
#[serde(tag = "op", content = "result", rename_all = "snake_case")]
pub enum NodeResponse {
    Create(StoredNode),
    Update(StoredNode),
    Delete(RuntimeDelete),
    Find(RuntimeNodeFindResponse),
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
#[serde(tag = "op", content = "result", rename_all = "snake_case")]
pub enum EdgeResponse {
    Create(StoredRel),
    Update(StoredRel),
    Delete(RuntimeDelete),
    Find(RuntimeEdgeFindResponse),
}

#[derive(Debug, Clone)]
pub struct RuntimeOperationOutcome<T> {
    pub value: T,
    pub durable_op: DurableOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDelete {
    pub model: String,
    pub id: i64,
}

pub type RuntimeNodeOutcome = RuntimeOperationOutcome<StoredNode>;
pub type RuntimeEdgeOutcome = RuntimeOperationOutcome<StoredRel>;
pub type RuntimeNodeDeleteOutcome = RuntimeOperationOutcome<RuntimeDelete>;
pub type RuntimeEdgeDeleteOutcome = RuntimeOperationOutcome<RuntimeDelete>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeNodeFindResponse {
    pub model: String,
    pub nodes: Vec<StoredNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEdgeFindResponse {
    pub model: String,
    pub edges: Vec<StoredRel>,
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

impl NodeFindRequest {
    /// Build a structured request from the legacy/simple filter-map shape used by adapters.
    ///
    /// This preserves MCP/Python compatibility for keys like `age>`, `title~`, and
    /// `order="age:asc"`. Future service boundaries should construct `NodeFindRequest`
    /// directly with `predicates`, `order`, `limit`, `offset`, and `id`.
    pub fn from_adapter_filter_values(
        model: impl Into<String>,
        filters: BTreeMap<String, Value>,
    ) -> Result<Self> {
        let mut request = Self {
            model: model.into(),
            ..Default::default()
        };

        for (raw_key, value) in filters {
            let raw_value = value_to_raw(&value)?;
            match raw_key.as_str() {
                "format" => {}
                "limit" => request.limit = Some(parse_usize_value(&raw_value, "limit")?),
                "offset" => request.offset = Some(parse_usize_value(&raw_value, "offset")?),
                "order" => request.order = parse_order_value(&raw_value)?,
                "id" => request.id = Some(parse_i64_value(&raw_value, "id")?),
                _ => {
                    let (field, op) = split_filter_key(&raw_key)?;
                    if field == "id" {
                        return Err(GrmError::Constraint(
                            "backend id filter 'id' only supports '='".into(),
                        ));
                    }
                    request.predicates.push(PropertyPredicate {
                        field: field.to_string(),
                        op,
                        value,
                    });
                }
            }
        }

        Ok(request)
    }
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

impl EdgeFindRequest {
    /// Build a structured request from the legacy/simple filter-map shape used by adapters.
    ///
    /// This preserves MCP/Python compatibility for keys like `year>`, `from`, `to`,
    /// and `order="year:desc"`. Future service boundaries should construct
    /// `EdgeFindRequest` directly with `predicates`, `order`, `limit`, `offset`,
    /// `id`, `from`, and `to`.
    pub fn from_adapter_filter_values(
        model: impl Into<String>,
        filters: BTreeMap<String, Value>,
    ) -> Result<Self> {
        let mut request = Self {
            model: model.into(),
            ..Default::default()
        };

        for (raw_key, value) in filters {
            let raw_value = value_to_raw(&value)?;
            match raw_key.as_str() {
                "format" => {}
                "limit" => request.limit = Some(parse_usize_value(&raw_value, "limit")?),
                "offset" => request.offset = Some(parse_usize_value(&raw_value, "offset")?),
                "order" => request.order = parse_order_value(&raw_value)?,
                "id" => request.id = Some(parse_i64_value(&raw_value, "id")?),
                "from" => request.from = Some(parse_i64_value(&raw_value, "from")?),
                "to" => request.to = Some(parse_i64_value(&raw_value, "to")?),
                _ => {
                    let (field, op) = split_filter_key(&raw_key)?;
                    if field == "id" || field == "from" || field == "to" {
                        return Err(GrmError::Constraint(format!(
                            "special filter '{field}' only supports '='"
                        )));
                    }
                    request.predicates.push(PropertyPredicate {
                        field: field.to_string(),
                        op,
                        value,
                    });
                }
            }
        }

        Ok(request)
    }
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
    IndexList,
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

fn split_filter_key(raw_key: &str) -> Result<(&str, PredicateOp)> {
    for (suffix, op) in [
        ("!", PredicateOp::Ne),
        (">=", PredicateOp::Ge),
        ("<=", PredicateOp::Le),
        (">", PredicateOp::Gt),
        ("<", PredicateOp::Lt),
        ("~", PredicateOp::Contains),
    ] {
        if let Some(field) = raw_key.strip_suffix(suffix) {
            if field.is_empty() {
                break;
            }
            return Ok((field, op));
        }
    }

    Ok((raw_key, PredicateOp::Eq))
}

fn parse_order_value(raw: &str) -> Result<Vec<OrderSpec>> {
    let mut order = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for segment in raw.split(',') {
        let Some((field, direction)) = segment.split_once(':') else {
            return Err(GrmError::Constraint(
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]".into(),
            ));
        };
        if !seen.insert(field.to_string()) {
            return Err(GrmError::Constraint(format!(
                "duplicate order field '{field}'"
            )));
        }
        let direction = match direction {
            "asc" => OrderDirection::Asc,
            "desc" => OrderDirection::Desc,
            _ => {
                return Err(GrmError::Constraint(
                    "order direction must be asc or desc".into(),
                ));
            }
        };
        order.push(OrderSpec {
            field: field.to_string(),
            direction,
        });
    }
    Ok(order)
}

fn parse_usize_value(raw: &str, field: &str) -> Result<usize> {
    raw.trim()
        .parse::<usize>()
        .map_err(|_| GrmError::Constraint(format!("{field} must be a non-negative integer")))
}

fn parse_i64_value(raw: &str, field: &str) -> Result<i64> {
    raw.trim()
        .parse::<i64>()
        .map_err(|_| GrmError::Constraint(format!("{field} must be an int id")))
}

fn value_to_raw(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null => Err(GrmError::Constraint(
            "null property values are not supported by runtime operations".into(),
        )),
        Value::Array(_) | Value::Object(_) => Err(GrmError::Constraint(
            "structured property values are not supported by runtime operations".into(),
        )),
    }
}
