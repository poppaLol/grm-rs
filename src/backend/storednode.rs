use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct StoredNode {
    pub id: i64,
    pub labels: Vec<String>,
    pub props: BTreeMap<String, Value>,
}