use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::error::{GrmError, Result};

pub fn to_props<T: Serialize>(value: &T) -> Result<BTreeMap<String, Value>> {
    let v = serde_json::to_value(value)
        .map_err(|e| GrmError::Mapping(format!("serialize props: {e}")))?;

    let obj = v.as_object()
        .ok_or_else(|| GrmError::Mapping("expected struct to serialize to object".into()))?;

    Ok(obj.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

pub fn from_props<T: DeserializeOwned>(props: BTreeMap<String, Value>) -> Result<T> {
    let v = Value::Object(props.into_iter().collect());
    serde_json::from_value(v)
        .map_err(|e| GrmError::Mapping(format!("deserialize props: {e}")))
}
