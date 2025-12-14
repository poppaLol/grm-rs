use serde_json::Value;

#[derive(Debug, Clone)]
pub struct QueryRow {
    pub values: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub rows: Vec<QueryRow>,
}