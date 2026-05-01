use std::collections::BTreeMap;

use grm_rs::{GrmError, Result as GrmResult, RuntimeField, RuntimeValueType};
use rmcp::ErrorData as McpError;
use rmcp::model::JsonObject;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FieldParam {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DefineNodeParams {
    pub name: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<FieldParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DefineEdgeParams {
    pub name: String,
    pub from_model: String,
    pub to_model: String,
    pub id_field: String,
    #[serde(default)]
    pub fields: Vec<FieldParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NodeCreateParams {
    pub model: String,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
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
