use std::collections::BTreeMap;

use crate::backend::{StoredNode, StoredRel};

#[derive(Debug, Clone)]
pub struct GraphStore {
    pub next_node_id: i64,
    pub next_rel_id: i64,
    pub nodes: BTreeMap<i64, StoredNode>,
    pub rels: BTreeMap<i64, StoredRel>,
}

impl Default for GraphStore {
    fn default() -> Self {
        Self {
            next_node_id: 1,
            next_rel_id: 1,
            nodes: BTreeMap::new(),
            rels: BTreeMap::new(),
        }
    }
}

impl GraphStore {
    pub fn clone_store(&self) -> Self {
        Self {
            next_node_id: self.next_node_id,
            next_rel_id: self.next_rel_id,
            nodes: self.nodes.clone(),
            rels: self.rels.clone(),
        }
    }
}
