use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{BackendIdType, GrmError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeValueType {
    String,
    Int,
    Float,
    Bool,
}

impl RuntimeValueType {
    pub fn parse_keyword(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "string" => Some(Self::String),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            _ => None,
        }
    }

    pub fn keyword(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
        }
    }

    pub fn parse_value(&self, input: &str) -> Result<Value> {
        match self {
            Self::String => Ok(Value::String(input.to_string())),
            Self::Int => input
                .trim()
                .parse::<i64>()
                .map(Value::from)
                .map_err(|_| GrmError::Constraint("expected int value".into())),
            Self::Float => input
                .trim()
                .parse::<f64>()
                .map(Value::from)
                .map_err(|_| GrmError::Constraint("expected float value".into())),
            Self::Bool => input
                .trim()
                .to_ascii_lowercase()
                .parse::<bool>()
                .map(Value::from)
                .map_err(|_| GrmError::Constraint("expected bool value (true/false)".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeField {
    pub name: String,
    pub value_type: RuntimeValueType,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSchemaOrigin {
    Declared,
    Inferred,
}

fn default_schema_origin() -> RuntimeSchemaOrigin {
    RuntimeSchemaOrigin::Declared
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeNodeModel {
    pub name: String,
    pub label: String,
    pub id_field_name: String,
    pub id_type: BackendIdType,
    #[serde(default = "default_schema_origin")]
    pub origin: RuntimeSchemaOrigin,
    pub fields: Vec<RuntimeField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRelModel {
    pub name: String,
    pub rel_type: String,
    pub from_model: String,
    pub to_model: String,
    pub id_field_name: String,
    pub id_type: BackendIdType,
    #[serde(default = "default_schema_origin")]
    pub origin: RuntimeSchemaOrigin,
    pub fields: Vec<RuntimeField>,
}

impl RuntimeNodeModel {
    pub fn new(
        name: impl Into<String>,
        id_field_name: impl Into<String>,
        id_type: BackendIdType,
        fields: Vec<RuntimeField>,
    ) -> Result<Self> {
        let name = name.into();
        let id_field_name = id_field_name.into();
        validate_model_name(&name)?;
        validate_field_name(&id_field_name)?;

        let mut seen = BTreeSet::new();
        seen.insert(id_field_name.clone());
        for field in &fields {
            validate_field_name(&field.name)?;
            if !seen.insert(field.name.clone()) {
                return Err(GrmError::Constraint(format!(
                    "field '{}' is defined more than once",
                    field.name
                )));
            }
        }

        Ok(Self {
            label: name.clone(),
            name,
            id_field_name,
            id_type,
            origin: RuntimeSchemaOrigin::Declared,
            fields,
        })
    }

    pub fn field(&self, name: &str) -> Option<&RuntimeField> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn validate_instance_input(
        &self,
        raw_values: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, Value>> {
        for key in raw_values.keys() {
            if self.field(key).is_none() {
                return Err(GrmError::Constraint(format!(
                    "unknown field '{}' for model '{}'",
                    key, self.name
                )));
            }
        }

        let mut props = BTreeMap::new();

        for field in &self.fields {
            match raw_values.get(&field.name) {
                Some(value) => {
                    let parsed = field.value_type.parse_value(value)?;
                    props.insert(field.name.clone(), parsed);
                }
                None if field.required => {
                    return Err(GrmError::Constraint(format!(
                        "missing required field '{}'",
                        field.name
                    )));
                }
                None => {}
            }
        }

        Ok(props)
    }
}

impl RuntimeRelModel {
    pub fn new(
        name: impl Into<String>,
        from_model: impl Into<String>,
        to_model: impl Into<String>,
        id_field_name: impl Into<String>,
        id_type: BackendIdType,
        fields: Vec<RuntimeField>,
    ) -> Result<Self> {
        let name = name.into();
        let from_model = from_model.into();
        let to_model = to_model.into();
        let id_field_name = id_field_name.into();

        validate_model_name(&name)?;
        validate_model_name(&from_model)?;
        validate_model_name(&to_model)?;
        validate_field_name(&id_field_name)?;

        let mut seen = BTreeSet::new();
        seen.insert(id_field_name.clone());
        for field in &fields {
            validate_field_name(&field.name)?;
            if !seen.insert(field.name.clone()) {
                return Err(GrmError::Constraint(format!(
                    "field '{}' is defined more than once",
                    field.name
                )));
            }
        }

        Ok(Self {
            rel_type: name.clone(),
            name,
            from_model,
            to_model,
            id_field_name,
            id_type,
            origin: RuntimeSchemaOrigin::Declared,
            fields,
        })
    }

    pub fn field(&self, name: &str) -> Option<&RuntimeField> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn validate_instance_input(
        &self,
        raw_values: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, Value>> {
        for key in raw_values.keys() {
            if self.field(key).is_none() {
                return Err(GrmError::Constraint(format!(
                    "unknown field '{}' for relationship model '{}'",
                    key, self.name
                )));
            }
        }

        let mut props = BTreeMap::new();

        for field in &self.fields {
            match raw_values.get(&field.name) {
                Some(value) => {
                    let parsed = field.value_type.parse_value(value)?;
                    props.insert(field.name.clone(), parsed);
                }
                None if field.required => {
                    return Err(GrmError::Constraint(format!(
                        "missing required field '{}'",
                        field.name
                    )));
                }
                None => {}
            }
        }

        Ok(props)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionModelCatalog {
    node_models: BTreeMap<String, RuntimeNodeModel>,
    rel_models: BTreeMap<String, RuntimeRelModel>,
}

impl SessionModelCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.node_models.is_empty() && self.rel_models.is_empty()
    }

    pub fn register_node_model(&mut self, model: RuntimeNodeModel) -> Result<()> {
        if self.node_models.contains_key(&model.name) || self.rel_models.contains_key(&model.name) {
            return Err(GrmError::Constraint(format!(
                "model '{}' already exists in this session",
                model.name
            )));
        }
        self.node_models.insert(model.name.clone(), model);
        Ok(())
    }

    pub fn register_rel_model(&mut self, model: RuntimeRelModel) -> Result<()> {
        if self.node_models.contains_key(&model.name) || self.rel_models.contains_key(&model.name) {
            return Err(GrmError::Constraint(format!(
                "model '{}' already exists in this session",
                model.name
            )));
        }
        self.rel_models.insert(model.name.clone(), model);
        Ok(())
    }

    pub fn register(&mut self, model: RuntimeNodeModel) -> Result<()> {
        self.register_node_model(model)
    }

    pub fn get_node_model(&self, name: &str) -> Option<&RuntimeNodeModel> {
        self.node_models.get(name)
    }

    pub fn get_rel_model(&self, name: &str) -> Option<&RuntimeRelModel> {
        self.rel_models.get(name)
    }

    pub fn get(&self, name: &str) -> Option<&RuntimeNodeModel> {
        self.get_node_model(name)
    }

    pub fn list_node_models(&self) -> Vec<&RuntimeNodeModel> {
        self.node_models.values().collect()
    }

    pub fn list_rel_models(&self) -> Vec<&RuntimeRelModel> {
        self.rel_models.values().collect()
    }

    pub fn list(&self) -> Vec<&RuntimeNodeModel> {
        self.list_node_models()
    }
}

pub fn validate_model_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(GrmError::Constraint("model name cannot be empty".into()));
    }

    let mut chars = name.chars();
    let first = chars
        .next()
        .ok_or_else(|| GrmError::Constraint("model name cannot be empty".into()))?;

    if !first.is_ascii_uppercase() {
        return Err(GrmError::Constraint(
            "model name must be PascalCase and start with an uppercase letter".into(),
        ));
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(GrmError::Constraint(
            "model name must contain only ASCII letters, digits, or underscores".into(),
        ));
    }

    Ok(())
}

pub fn validate_field_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(GrmError::Constraint("field name cannot be empty".into()));
    }
    if name == "id" {
        return Err(GrmError::Constraint(
            "field name 'id' is reserved and cannot be used".into(),
        ));
    }

    let mut chars = name.chars();
    let first = chars
        .next()
        .ok_or_else(|| GrmError::Constraint("field name cannot be empty".into()))?;

    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(GrmError::Constraint(
            "field name must start with a letter or underscore".into(),
        ));
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(GrmError::Constraint(
            "field name must contain only ASCII letters, digits, or underscores".into(),
        ));
    }

    Ok(())
}

pub fn parse_required_flag(input: &str) -> Option<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" | "required" => Some(true),
        "n" | "no" | "optional" => Some(false),
        _ => None,
    }
}
