use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    DefineEdgeRequest, DefineNodeRequest, DurableOperation, EdgeCreateRequest, EdgeDeleteRequest,
    EdgeUpdateRequest, FieldSpec, FieldValueType, GrmError, NodeCreateRequest, NodeDeleteRequest,
    NodeUpdateRequest, Result, SessionState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchFieldParam {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchDefineNodeParams {
    pub name: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<SessionBatchFieldParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchDefineEdgeParams {
    pub name: String,
    pub from_model: String,
    pub to_model: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<SessionBatchFieldParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchNodeCreateParams {
    pub model: String,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(default, rename = "ref")]
    pub local_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchNodeUpdateParams {
    pub model: String,
    pub id: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchNodeDeleteParams {
    pub model: String,
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionBatchEndpoint {
    Id(i64),
    Ref(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchEdgeCreateParams {
    pub model: String,
    pub from: SessionBatchEndpoint,
    pub to: SessionBatchEndpoint,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchEdgeUpdateParams {
    pub model: String,
    pub id: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchEdgeDeleteParams {
    pub model: String,
    pub id: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionBatchResponse {
    Summary,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum SessionBatchOp {
    SchemaDefineNode(SessionBatchDefineNodeParams),
    SchemaDefineEdge(SessionBatchDefineEdgeParams),
    NodeCreate(SessionBatchNodeCreateParams),
    NodeUpdate(SessionBatchNodeUpdateParams),
    NodeDelete(SessionBatchNodeDeleteParams),
    EdgeCreate(SessionBatchEdgeCreateParams),
    EdgeUpdate(SessionBatchEdgeUpdateParams),
    EdgeDelete(SessionBatchEdgeDeleteParams),
}

impl SessionBatchOp {
    pub fn op_name(&self) -> &'static str {
        match self {
            Self::SchemaDefineNode(_) => "schema_define_node",
            Self::SchemaDefineEdge(_) => "schema_define_edge",
            Self::NodeCreate(_) => "node_create",
            Self::NodeUpdate(_) => "node_update",
            Self::NodeDelete(_) => "node_delete",
            Self::EdgeCreate(_) => "edge_create",
            Self::EdgeUpdate(_) => "edge_update",
            Self::EdgeDelete(_) => "edge_delete",
        }
    }

    pub fn is_delete(&self) -> bool {
        matches!(self, Self::NodeDelete(_) | Self::EdgeDelete(_))
    }
}

fn default_atomic() -> bool {
    true
}

fn default_batch_response() -> SessionBatchResponse {
    SessionBatchResponse::Summary
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBatchParams {
    #[serde(default = "default_atomic")]
    pub atomic: bool,
    #[serde(default)]
    pub allow_deletes: bool,
    #[serde(default = "default_batch_response")]
    pub response: SessionBatchResponse,
    pub ops: Vec<SessionBatchOp>,
}

#[derive(Debug, Clone)]
pub struct SessionBatchOutcome {
    pub value: Value,
    pub should_persist: bool,
    pub durable_ops: Vec<DurableOperation>,
}

struct BatchApplied {
    op: &'static str,
    model: String,
    id: Option<i64>,
    local_ref: Option<String>,
    durable_op: DurableOperation,
}

struct BatchSummary {
    applied: bool,
    atomic: bool,
    detailed: bool,
    operation_count: usize,
    counts: BTreeMap<String, BTreeMap<String, usize>>,
    errors: Vec<Value>,
    ids: Vec<Value>,
}

impl BatchSummary {
    fn new(atomic: bool, detailed: bool, operation_count: usize) -> Self {
        Self {
            applied: true,
            atomic,
            detailed,
            operation_count,
            counts: BTreeMap::new(),
            errors: Vec::new(),
            ids: Vec::new(),
        }
    }

    fn record(&mut self, applied: BatchApplied) {
        *self
            .counts
            .entry(applied.op.to_string())
            .or_default()
            .entry(applied.model.clone())
            .or_default() += 1;

        if self.detailed {
            if let Some(id) = applied.id {
                let mut value = json!({
                    "op": applied.op,
                    "model": applied.model,
                    "id": id,
                });
                if let Some(local_ref) = applied.local_ref {
                    value["ref"] = json!(local_ref);
                }
                self.ids.push(value);
            }
        }
    }

    fn record_error(&mut self, index: usize, message: String) {
        self.applied = false;
        self.errors.push(json!({
            "index": index,
            "message": message,
            "recovery": "Inspect the operation at this index, call grm_schema_list if model fields or ids are uncertain, then retry the failed operation."
        }));
    }

    fn has_successes(&self) -> bool {
        !self.counts.is_empty()
    }

    fn into_value(self) -> Value {
        let mut value = json!({
            "applied": self.applied,
            "atomic": self.atomic,
            "operation_count": self.operation_count,
            "counts": self.counts,
            "errors": self.errors,
        });
        if self.detailed {
            value["ids"] = json!(self.ids);
        }
        value
    }
}

pub async fn apply_session_batch(
    state: &mut SessionState,
    params: SessionBatchParams,
) -> Result<SessionBatchOutcome> {
    let snapshot = params.atomic.then(|| state.snapshot());
    let mut summary = BatchSummary::new(
        params.atomic,
        matches!(params.response, SessionBatchResponse::Detailed),
        params.ops.len(),
    );
    let mut refs = BTreeMap::<String, i64>::new();
    let mut durable_ops = Vec::new();

    for (index, op) in params.ops.into_iter().enumerate() {
        if op.is_delete() && !params.allow_deletes {
            summary.record_error(
                index,
                format!("{} requires allow_deletes=true on grm_batch", op.op_name()),
            );
            if params.atomic {
                if let Some(snapshot) = snapshot {
                    state.restore(snapshot);
                }
                summary.applied = false;
                return Ok(SessionBatchOutcome {
                    durable_ops: Vec::new(),
                    should_persist: false,
                    value: summary.into_value(),
                });
            }
            continue;
        }

        let result = apply_batch_op(state, &mut refs, op).await;
        match result {
            Ok(applied) => {
                durable_ops.push(applied.durable_op.clone());
                summary.record(applied);
            }
            Err(err) => {
                summary.record_error(index, err.to_string());
                if params.atomic {
                    if let Some(snapshot) = snapshot {
                        state.restore(snapshot);
                    }
                    summary.applied = false;
                    return Ok(SessionBatchOutcome {
                        durable_ops: Vec::new(),
                        should_persist: false,
                        value: summary.into_value(),
                    });
                }
            }
        }
    }

    let should_persist = summary.applied || summary.has_successes();
    let durable_ops = match durable_ops.len() {
        0 => durable_ops,
        1 => durable_ops,
        _ => vec![DurableOperation::Batch { ops: durable_ops }],
    };
    Ok(SessionBatchOutcome {
        durable_ops,
        value: summary.into_value(),
        should_persist,
    })
}

async fn apply_batch_op(
    state: &mut SessionState,
    refs: &mut BTreeMap<String, i64>,
    op: SessionBatchOp,
) -> Result<BatchApplied> {
    let op_name = op.op_name();
    match op {
        SessionBatchOp::SchemaDefineNode(params) => {
            let outcome = state.apply_define_node(DefineNodeRequest {
                name: params.name,
                id_field: params.id_field,
                fields: parse_fields(params.fields)?,
            })?;
            Ok(BatchApplied {
                op: op_name,
                model: outcome.value.name,
                id: None,
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::SchemaDefineEdge(params) => {
            let outcome = state.apply_define_edge(DefineEdgeRequest {
                name: params.name,
                from_model: params.from_model,
                to_model: params.to_model,
                id_field: params.id_field,
                fields: parse_fields(params.fields)?,
            })?;
            Ok(BatchApplied {
                op: op_name,
                model: outcome.value.name,
                id: None,
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::NodeCreate(params) => {
            if let Some(local_ref) = &params.local_ref {
                if refs.contains_key(local_ref) {
                    return Err(GrmError::Constraint(format!(
                        "duplicate batch ref '{local_ref}'"
                    )));
                }
            }
            let model = params.model;
            let outcome = state
                .apply_node_create(NodeCreateRequest {
                    model: model.clone(),
                    props: params.props,
                })
                .await?;
            let node = outcome.value;
            if let Some(local_ref) = &params.local_ref {
                refs.insert(local_ref.clone(), node.id);
            }
            Ok(BatchApplied {
                op: op_name,
                model,
                id: Some(node.id),
                local_ref: params.local_ref,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::NodeUpdate(params) => {
            let model = params.model;
            let outcome = state
                .apply_node_update(NodeUpdateRequest {
                    model: model.clone(),
                    id: params.id,
                    props: params.props,
                })
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model,
                id: Some(outcome.value.id),
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::NodeDelete(params) => {
            let outcome = state
                .apply_node_delete(NodeDeleteRequest {
                    model: params.model,
                    id: params.id,
                })
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: outcome.value.model,
                id: Some(outcome.value.id),
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::EdgeCreate(params) => {
            let from = resolve_batch_endpoint(&params.from, refs, "from")?;
            let to = resolve_batch_endpoint(&params.to, refs, "to")?;
            let model = params.model;
            let outcome = state
                .apply_edge_create(EdgeCreateRequest {
                    model: model.clone(),
                    from,
                    to,
                    props: params.props,
                })
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model,
                id: Some(outcome.value.id),
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::EdgeUpdate(params) => {
            let model = params.model;
            let outcome = state
                .apply_edge_update(EdgeUpdateRequest {
                    model: model.clone(),
                    id: params.id,
                    props: params.props,
                })
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model,
                id: Some(outcome.value.id),
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
        SessionBatchOp::EdgeDelete(params) => {
            let outcome = state
                .apply_edge_delete(EdgeDeleteRequest {
                    model: params.model,
                    id: params.id,
                })
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: outcome.value.model,
                id: Some(outcome.value.id),
                local_ref: None,
                durable_op: outcome.durable_op,
            })
        }
    }
}

fn parse_fields(fields: Vec<SessionBatchFieldParam>) -> Result<Vec<FieldSpec>> {
    fields
        .into_iter()
        .map(|field| {
            let value_type = parse_field_value_type(&field.value_type).ok_or_else(|| {
                GrmError::Constraint(format!(
                    "unsupported field type '{}', expected one of: string, int, float, bool",
                    field.value_type
                ))
            })?;
            Ok(FieldSpec {
                name: field.name,
                value_type,
                required: field.required,
            })
        })
        .collect()
}

fn parse_field_value_type(raw: &str) -> Option<FieldValueType> {
    match raw {
        "string" => Some(FieldValueType::String),
        "int" => Some(FieldValueType::Int),
        "float" => Some(FieldValueType::Float),
        "bool" => Some(FieldValueType::Bool),
        _ => None,
    }
}

fn resolve_batch_endpoint(
    endpoint: &SessionBatchEndpoint,
    refs: &BTreeMap<String, i64>,
    field: &str,
) -> Result<i64> {
    match endpoint {
        SessionBatchEndpoint::Id(id) => Ok(*id),
        SessionBatchEndpoint::Ref(local_ref) => refs.get(local_ref).copied().ok_or_else(|| {
            GrmError::Constraint(format!(
                "{field} ref '{local_ref}' was not created earlier in this batch"
            ))
        }),
    }
}
