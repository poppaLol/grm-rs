use std::collections::BTreeMap;

use crate::{
    GraphQuery, KernelValue,
    dsl::{Return, VarId, graph::Value},
};

#[derive(Debug, Clone)]
pub struct QueryRow {
    /// Values bound by a backend for one `GraphQuery` result row.
    ///
    /// Backend contract:
    /// - keys are kernel `VarId`s introduced by `GraphQuery::bound_vars()`
    /// - every returned row must include all variables bound by the executed
    ///   graph query
    /// - every returned row must include `GraphQuery::return_var()`
    /// - the returned value variant must match `GraphQuery::return_kind()`
    pub values: BTreeMap<VarId, Value>,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Ordered query rows. Empty means the query matched no rows.
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
