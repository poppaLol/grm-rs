use std::collections::BTreeMap;

use crate::{GraphQuery, KernelValue, dsl::{Return, VarId, graph::Value}};

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

    pub fn contains_key(&self, v: &VarId) -> bool {
        self.values.contains_key(v)
    }

    pub fn get(&self, v: &VarId) -> Option<&KernelValue> {
        self.values.get(v)
    }

    pub fn keys(&self) -> impl Iterator<Item = &VarId> {
        self.values.keys()
    }
}
