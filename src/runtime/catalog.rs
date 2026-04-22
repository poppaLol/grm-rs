use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{BackendIdType, GrmError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
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
                .parse::<bool>()
                .map(Value::from)
                .map_err(|_| GrmError::Constraint("expected bool value (true/false)".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeField {
    pub name: String,
    pub value_type: RuntimeValueType,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeNodeModel {
    pub name: String,
    pub label: String,
    pub id_field_name: String,
    pub id_type: BackendIdType,
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

#[derive(Debug, Clone, Default)]
pub struct SessionModelCatalog {
    models: BTreeMap<String, RuntimeNodeModel>,
}

impl SessionModelCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    pub fn register(&mut self, model: RuntimeNodeModel) -> Result<()> {
        if self.models.contains_key(&model.name) {
            return Err(GrmError::Constraint(format!(
                "model '{}' already exists in this session",
                model.name
            )));
        }
        self.models.insert(model.name.clone(), model);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RuntimeNodeModel> {
        self.models.get(name)
    }

    pub fn list(&self) -> Vec<&RuntimeNodeModel> {
        self.models.values().collect()
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

    if !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
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

    if !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
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
