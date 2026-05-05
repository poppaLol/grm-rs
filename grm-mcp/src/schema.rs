use std::collections::BTreeMap;

use grm_rs::{GrmError, Result as GrmResult, RuntimeField, RuntimeValueType};
use rmcp::ErrorData as McpError;
use rmcp::model::JsonObject;
use rmcp::schemars;
use rmcp::schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldParam {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub required: bool,
}

impl JsonSchema for FieldParam {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FieldParam".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        field_schema_value()
            .try_into()
            .expect("valid FieldParam schema")
    }
}

fn field_schema_value() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "Runtime property field name."
            },
            "type": {
                "type": "string",
                "enum": ["string", "int", "float", "bool"],
                "description": "Runtime property value type."
            },
            "required": {
                "type": "boolean",
                "description": "Whether this field must be present when creating an instance."
            }
        },
        "required": ["name", "type", "required"],
        "additionalProperties": false
    })
}

fn fields_schema() -> Value {
    json!({
        "type": "array",
        "default": [],
        "items": field_schema_value()
    })
}

fn props_schema() -> Value {
    json!({
        "type": "object",
        "default": {},
        "additionalProperties": {
            "anyOf": [
                { "type": "string" },
                { "type": "number" },
                { "type": "boolean" }
            ]
        }
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefineNodeParams {
    pub name: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<FieldParam>,
}

impl JsonSchema for DefineNodeParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DefineNodeParams".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "id_field": { "type": "string" },
                "fields": fields_schema()
            },
            "required": ["name", "id_field"],
            "additionalProperties": false
        })
        .try_into()
        .expect("valid DefineNodeParams schema")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefineEdgeParams {
    pub name: String,
    pub from_model: String,
    pub to_model: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<FieldParam>,
}

impl JsonSchema for DefineEdgeParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DefineEdgeParams".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "from_model": { "type": "string" },
                "to_model": { "type": "string" },
                "id_field": { "type": "string" },
                "fields": fields_schema()
            },
            "required": ["name", "from_model", "to_model", "id_field"],
            "additionalProperties": false
        })
        .try_into()
        .expect("valid DefineEdgeParams schema")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NodeCreateParams {
    pub model: String,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchNodeCreateParams {
    pub model: String,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(default, rename = "ref")]
    pub local_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NodeUpdateParams {
    pub model: String,
    pub id: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NodeDeleteParams {
    pub model: String,
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NodeFindParams {
    pub model: String,
    #[serde(default)]
    pub filters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EdgeCreateParams {
    pub model: String,
    pub from: i64,
    pub to: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum BatchEndpoint {
    Id(i64),
    Ref(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchEdgeCreateParams {
    pub model: String,
    pub from: BatchEndpoint,
    pub to: BatchEndpoint,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EdgeUpdateParams {
    pub model: String,
    pub id: i64,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EdgeDeleteParams {
    pub model: String,
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EdgeFindParams {
    pub model: String,
    #[serde(default)]
    pub filters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueryParams {
    pub command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Json,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileFormatParams {
    pub format: FileFormat,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PathParams {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExportParams {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolHelpParams {
    pub tool: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatchResponse {
    Summary,
    Detailed,
}

fn default_atomic() -> bool {
    true
}

fn default_batch_response() -> BatchResponse {
    BatchResponse::Summary
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum BatchOp {
    SchemaDefineNode(DefineNodeParams),
    SchemaDefineEdge(DefineEdgeParams),
    NodeCreate(BatchNodeCreateParams),
    NodeUpdate(NodeUpdateParams),
    NodeDelete(NodeDeleteParams),
    EdgeCreate(BatchEdgeCreateParams),
    EdgeUpdate(EdgeUpdateParams),
    EdgeDelete(EdgeDeleteParams),
}

impl JsonSchema for BatchOp {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BatchOp".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        batch_op_schema().try_into().expect("valid BatchOp schema")
    }
}

impl BatchOp {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchParams {
    #[serde(default = "default_atomic")]
    pub atomic: bool,
    #[serde(default)]
    pub allow_deletes: bool,
    #[serde(default = "default_batch_response")]
    pub response: BatchResponse,
    pub ops: Vec<BatchOp>,
}

impl JsonSchema for BatchParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BatchParams".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json!({
            "type": "object",
            "properties": {
                "atomic": {
                    "type": "boolean",
                    "default": true,
                    "description": "Whether the batch should apply all operations or roll back all successful operations after the first failure."
                },
                "allow_deletes": {
                    "type": "boolean",
                    "default": false,
                    "description": "Must be true for node_delete or edge_delete operations to run."
                },
                "response": {
                    "type": "string",
                    "enum": ["summary", "detailed"],
                    "default": "summary",
                    "description": "Use detailed to include created, updated, or deleted ids in the response."
                },
                "ops": {
                    "type": "array",
                    "description": "Ordered structured schema/node/edge operations.",
                    "items": batch_op_schema()
                }
            },
            "required": ["ops"],
            "additionalProperties": false
        })
        .try_into()
        .expect("valid BatchParams schema")
    }
}

fn object_schema(properties: Value, required: Vec<&str>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn batch_op_variant_schema(op: &str, args: Value) -> Value {
    object_schema(
        json!({
            "op": {
                "type": "string",
                "enum": [op]
            },
            "args": args
        }),
        vec!["op", "args"],
    )
}

fn batch_op_schema() -> Value {
    let define_node_args = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "id_field": { "type": "string" },
            "fields": fields_schema()
        },
        "required": ["name", "id_field"],
        "additionalProperties": false
    });
    let define_edge_args = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "from_model": { "type": "string" },
            "to_model": { "type": "string" },
            "id_field": { "type": "string" },
            "fields": fields_schema()
        },
        "required": ["name", "from_model", "to_model", "id_field"],
        "additionalProperties": false
    });
    let node_create_args = object_schema(
        json!({
            "ref": {
                "type": "string",
                "description": "Batch-local reference that later edge_create operations may use as an endpoint."
            },
            "model": { "type": "string" },
            "props": props_schema()
        }),
        vec!["model"],
    );
    let node_update_args = object_schema(
        json!({
            "model": { "type": "string" },
            "id": { "type": "integer", "format": "int64" },
            "props": props_schema()
        }),
        vec!["model", "id"],
    );
    let node_delete_args = object_schema(
        json!({
            "model": { "type": "string" },
            "id": { "type": "integer", "format": "int64" }
        }),
        vec!["model", "id"],
    );
    let endpoint_schema = json!({
        "anyOf": [
            { "type": "integer", "format": "int64" },
            { "type": "string" }
        ],
        "description": "Numeric node id or batch-local ref created by an earlier node_create operation."
    });
    let edge_create_args = object_schema(
        json!({
            "model": { "type": "string" },
            "from": endpoint_schema.clone(),
            "to": endpoint_schema,
            "props": props_schema()
        }),
        vec!["model", "from", "to"],
    );
    let edge_update_args = object_schema(
        json!({
            "model": { "type": "string" },
            "id": { "type": "integer", "format": "int64" },
            "props": props_schema()
        }),
        vec!["model", "id"],
    );
    let edge_delete_args = object_schema(
        json!({
            "model": { "type": "string" },
            "id": { "type": "integer", "format": "int64" }
        }),
        vec!["model", "id"],
    );

    json!({
        "oneOf": [
            batch_op_variant_schema("schema_define_node", define_node_args),
            batch_op_variant_schema("schema_define_edge", define_edge_args),
            batch_op_variant_schema("node_create", node_create_args),
            batch_op_variant_schema("node_update", node_update_args),
            batch_op_variant_schema("node_delete", node_delete_args),
            batch_op_variant_schema("edge_create", edge_create_args),
            batch_op_variant_schema("edge_update", edge_update_args),
            batch_op_variant_schema("edge_delete", edge_delete_args)
        ]
    })
}

pub(crate) fn parse_fields(fields: Vec<FieldParam>) -> GrmResult<Vec<RuntimeField>> {
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

pub(crate) fn value_map_to_raw(
    values: BTreeMap<String, Value>,
) -> GrmResult<BTreeMap<String, String>> {
    values
        .into_iter()
        .map(|(key, value)| Ok((key, value_to_raw(value)?)))
        .collect()
}

pub(crate) fn json_error(err: serde_json::Error) -> GrmError {
    GrmError::Backend(err.to_string())
}

pub(crate) fn to_object(value: Value) -> Result<JsonObject, McpError> {
    match value {
        Value::Object(object) => Ok(object),
        other => Err(McpError::internal_error(
            "tool result was not a JSON object",
            Some(json!({ "result": other })),
        )),
    }
}

fn value_to_raw(value: Value) -> GrmResult<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_values_to_runtime_strings() {
        let raw = value_map_to_raw(BTreeMap::from([
            ("name".to_string(), json!("Alice")),
            ("age".to_string(), json!(42)),
            ("published".to_string(), json!(true)),
        ]))
        .unwrap();

        assert_eq!(raw["name"], "Alice");
        assert_eq!(raw["age"], "42");
        assert_eq!(raw["published"], "true");
    }
}
