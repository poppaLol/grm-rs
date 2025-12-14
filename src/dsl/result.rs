use std::collections::BTreeMap;

use serde_json::Value;

use crate::{GraphQuery, dsl::VarId};

#[derive(Debug, Clone)]
pub struct QueryRow {
    pub values: BTreeMap<VarId, Value>,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub rows: Vec<QueryRow>,
}

impl QueryRow {
    #[inline]
    pub fn get(&self, var: VarId) -> Option<&serde_json::Value> {
        self.values.get(&var)
    }

    /// Convenience for your current “single return” world:
    #[inline]
    pub fn get_returned(&self, q: &GraphQuery) -> Option<&serde_json::Value> {
        self.get(q.return_var())
    }
}
