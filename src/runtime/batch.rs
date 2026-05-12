use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    GrmError, Result, RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeValueType,
    SessionState,
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
}

struct BatchApplied {
    op: &'static str,
    model: String,
    id: Option<i64>,
    local_ref: Option<String>,
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
                    should_persist: false,
                    value: summary.into_value(),
                });
            }
            continue;
        }

        let result = apply_batch_op(state, &mut refs, op).await;
        match result {
            Ok(applied) => summary.record(applied),
            Err(err) => {
                summary.record_error(index, err.to_string());
                if params.atomic {
                    if let Some(snapshot) = snapshot {
                        state.restore(snapshot);
                    }
                    summary.applied = false;
                    return Ok(SessionBatchOutcome {
                        should_persist: false,
                        value: summary.into_value(),
                    });
                }
            }
        }
    }

    let should_persist = summary.applied || summary.has_successes();
    Ok(SessionBatchOutcome {
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
            let model = RuntimeNodeModel::new(
                params.name.clone(),
                params.id_field,
                state.node_id_type(),
                parse_fields(params.fields)?,
            )?;
            state.register_model(model)?;
            Ok(BatchApplied {
                op: op_name,
                model: params.name,
                id: None,
                local_ref: None,
            })
        }
        SessionBatchOp::SchemaDefineEdge(params) => {
            let model = RuntimeRelModel::new(
                params.name.clone(),
                params.from_model,
                params.to_model,
                params.id_field,
                state.rel_id_type(),
                parse_fields(params.fields)?,
            )?;
            state.register_rel_model(model)?;
            Ok(BatchApplied {
                op: op_name,
                model: params.name,
                id: None,
                local_ref: None,
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
            let props = value_map_to_raw(params.props)?;
            let node = state.create_instance(&params.model, &props).await?;
            if let Some(local_ref) = &params.local_ref {
                refs.insert(local_ref.clone(), node.id);
            }
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(node.id),
                local_ref: params.local_ref,
            })
        }
        SessionBatchOp::NodeUpdate(params) => {
            let props = value_map_to_raw(params.props)?;
            let node = state
                .update_node_instance(&params.model, &params.id.to_string(), &props)
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(node.id),
                local_ref: None,
            })
        }
        SessionBatchOp::NodeDelete(params) => {
            state
                .delete_node_instance(&params.model, &params.id.to_string())
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(params.id),
                local_ref: None,
            })
        }
        SessionBatchOp::EdgeCreate(params) => {
            let from = resolve_batch_endpoint(&params.from, refs, "from")?;
            let to = resolve_batch_endpoint(&params.to, refs, "to")?;
            let props = value_map_to_raw(params.props)?;
            let edge = state
                .create_relationship_instance(
                    &params.model,
                    &from.to_string(),
                    &to.to_string(),
                    &props,
                )
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(edge.id),
                local_ref: None,
            })
        }
        SessionBatchOp::EdgeUpdate(params) => {
            let props = value_map_to_raw(params.props)?;
            let edge = state
                .update_relationship_instance(&params.model, &params.id.to_string(), &props)
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(edge.id),
                local_ref: None,
            })
        }
        SessionBatchOp::EdgeDelete(params) => {
            state
                .delete_relationship_instance(&params.model, &params.id.to_string())
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(params.id),
                local_ref: None,
            })
        }
    }
}

fn parse_fields(fields: Vec<SessionBatchFieldParam>) -> Result<Vec<RuntimeField>> {
    fields
        .into_iter()
        .map(|field| {
            let value_type =
                RuntimeValueType::parse_keyword(&field.value_type).ok_or_else(|| {
                    GrmError::Constraint(format!(
                        "unsupported field type '{}', expected one of: string, int, float, bool",
                        field.value_type
                    ))
                })?;
            Ok(RuntimeField {
                name: field.name,
                value_type,
                required: field.required,
            })
        })
        .collect()
}

fn value_map_to_raw(values: BTreeMap<String, Value>) -> Result<BTreeMap<String, String>> {
    values
        .into_iter()
        .map(|(key, value)| Ok((key, value_to_raw(value)?)))
        .collect()
}

fn value_to_raw(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null => Err(GrmError::Constraint(
            "null is not a supported graph value; omit the field instead".into(),
        )),
        Value::Array(_) | Value::Object(_) => Err(GrmError::Constraint(
            "graph values must be strings, numbers, or booleans".into(),
        )),
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
