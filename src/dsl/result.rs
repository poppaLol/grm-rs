use std::collections::BTreeMap;

use crate::{GraphQuery, dsl::{Return, VarId, graph::Value}};

#[derive(Debug, Clone)]
pub struct QueryRow {
    pub values: BTreeMap<VarId, Value>,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub rows: Vec<QueryRow>,
}

impl QueryRow {
    pub fn get_returned(&self, gq: &GraphQuery) -> Option<&Value> {
        match gq.ret {
            Return::Node(var) | Return::Rel(var) => self.values.get(&var),
        }
    }
}
