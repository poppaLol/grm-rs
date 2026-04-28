use std::collections::{BTreeMap, BTreeSet};

use crate::backend::{StoredNode, StoredRel};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueKey {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    FloatBits(u64),
    String(String),
}

impl ValueKey {
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::Null),
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Number(value) => value
                .as_i64()
                .map(Self::Int)
                .or_else(|| value.as_u64().map(Self::UInt))
                .or_else(|| value.as_f64().map(|value| Self::FloatBits(value.to_bits()))),
            Value::String(value) => Some(Self::String(value.clone())),
            Value::Array(_) | Value::Object(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GraphStore {
    pub next_node_id: i64,
    pub next_rel_id: i64,
    pub nodes: BTreeMap<i64, StoredNode>,
    pub rels: BTreeMap<i64, StoredRel>,
    pub nodes_by_label: BTreeMap<String, BTreeSet<i64>>,
    // `node_props` is a derived cache, not source-of-truth graph data.
    //
    // Node writes update `nodes` and `nodes_by_label` immediately, then mark this
    // property index dirty. Property-indexed reads must go through
    // `node_ids_by_label_property`, which rebuilds the cache first when dirty.
    // This preserves read-your-writes and transaction isolation while avoiding
    // high-cardinality property-index churn on insert-heavy workloads.
    //
    // Do not read `node_props` directly from new code; doing so can observe a
    // stale cache. If these fields become private later, this is the first one
    // that should be hidden behind methods.
    pub node_props: BTreeMap<(String, String, ValueKey), BTreeSet<i64>>,
    pub node_props_dirty: bool,
    pub rels_by_type: BTreeMap<String, BTreeSet<i64>>,
    pub outgoing: BTreeMap<(i64, String), BTreeSet<i64>>,
    pub incoming: BTreeMap<(i64, String), BTreeSet<i64>>,
    pub outgoing_any: BTreeMap<i64, BTreeSet<i64>>,
    pub incoming_any: BTreeMap<i64, BTreeSet<i64>>,
}

impl Default for GraphStore {
    fn default() -> Self {
        Self {
            next_node_id: 1,
            next_rel_id: 1,
            nodes: BTreeMap::new(),
            rels: BTreeMap::new(),
            nodes_by_label: BTreeMap::new(),
            node_props: BTreeMap::new(),
            node_props_dirty: false,
            rels_by_type: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing_any: BTreeMap::new(),
            incoming_any: BTreeMap::new(),
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
            nodes_by_label: self.nodes_by_label.clone(),
            node_props: self.node_props.clone(),
            node_props_dirty: self.node_props_dirty,
            rels_by_type: self.rels_by_type.clone(),
            outgoing: self.outgoing.clone(),
            incoming: self.incoming.clone(),
            outgoing_any: self.outgoing_any.clone(),
            incoming_any: self.incoming_any.clone(),
        }
    }

    pub fn rebuild_indexes(&mut self) {
        self.nodes_by_label.clear();
        self.node_props.clear();
        self.rels_by_type.clear();
        self.outgoing.clear();
        self.incoming.clear();
        self.outgoing_any.clear();
        self.incoming_any.clear();

        for node in self.nodes.values().cloned().collect::<Vec<_>>() {
            self.index_node_labels(&node);
            self.index_node_props(&node);
        }
        for rel in self.rels.values().cloned().collect::<Vec<_>>() {
            self.index_relationship(&rel);
        }
        self.node_props_dirty = false;
    }

    pub fn insert_node(&mut self, id: i64, node: StoredNode) -> Option<StoredNode> {
        let previous = self.nodes.insert(id, node.clone());
        if let Some(previous) = &previous {
            self.deindex_node(previous);
        }
        self.index_node_labels(&node);
        self.node_props_dirty = true;
        previous
    }

    pub fn insert_node_deferred_property_index(
        &mut self,
        id: i64,
        node: StoredNode,
    ) -> Option<StoredNode> {
        let previous = self.nodes.insert(id, node.clone());
        if let Some(previous) = &previous {
            self.deindex_node(previous);
        }
        self.index_node_labels(&node);
        self.node_props_dirty = true;
        previous
    }

    pub fn rebuild_node_property_index(&mut self) {
        self.node_props.clear();
        for node in self.nodes.values().cloned().collect::<Vec<_>>() {
            self.index_node_props(&node);
        }
        self.node_props_dirty = false;
    }

    pub fn remove_node(&mut self, id: i64) -> Option<StoredNode> {
        let removed = self.nodes.remove(&id);
        if removed.is_some() {
            if let Some(node) = &removed {
                self.deindex_node(node);
                self.node_props_dirty = true;
            }
            let rel_ids = self
                .rels
                .values()
                .filter(|rel| rel.from == id || rel.to == id)
                .map(|rel| rel.id)
                .collect::<Vec<_>>();
            for rel_id in rel_ids {
                self.remove_relationship(rel_id);
            }
        }
        removed
    }

    pub fn insert_relationship(&mut self, id: i64, rel: StoredRel) -> Option<StoredRel> {
        let previous = self.rels.insert(id, rel.clone());
        if let Some(previous) = &previous {
            self.deindex_relationship(previous);
        }
        self.index_relationship(&rel);
        previous
    }

    pub fn remove_relationship(&mut self, id: i64) -> Option<StoredRel> {
        let removed = self.rels.remove(&id);
        if let Some(rel) = &removed {
            self.deindex_relationship(rel);
        }
        removed
    }

    pub fn outgoing_relationship_ids(&self, from: i64, rel_type: Option<&str>) -> Vec<i64> {
        match rel_type {
            Some(rel_type) => self
                .outgoing
                .get(&(from, rel_type.to_string()))
                .map(|ids| ids.iter().copied().collect())
                .unwrap_or_default(),
            None => self
                .outgoing_any
                .get(&from)
                .map(|ids| ids.iter().copied().collect())
                .unwrap_or_default(),
        }
    }

    pub fn incoming_relationship_ids(&self, to: i64, rel_type: Option<&str>) -> Vec<i64> {
        match rel_type {
            Some(rel_type) => self
                .incoming
                .get(&(to, rel_type.to_string()))
                .map(|ids| ids.iter().copied().collect())
                .unwrap_or_default(),
            None => self
                .incoming_any
                .get(&to)
                .map(|ids| ids.iter().copied().collect())
                .unwrap_or_default(),
        }
    }

    pub fn node_ids_by_label(&self, label: &str) -> Vec<i64> {
        self.nodes_by_label
            .get(label)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn node_ids_by_label_property(
        &mut self,
        label: &str,
        key: &str,
        value: &Value,
    ) -> Vec<i64> {
        // This is the only supported access path for `node_props`: it keeps the
        // derived cache coherent before answering property-indexed reads.
        if self.node_props_dirty {
            self.rebuild_node_property_index();
        }
        let Some(value_key) = ValueKey::from_value(value) else {
            return Vec::new();
        };
        self.node_props
            .get(&(label.to_string(), key.to_string(), value_key))
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn relationship_ids_by_type(&self, rel_type: &str) -> Vec<i64> {
        self.rels_by_type
            .get(rel_type)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default()
    }

    fn index_node_labels(&mut self, node: &StoredNode) {
        for label in &node.labels {
            self.nodes_by_label
                .entry(label.clone())
                .or_default()
                .insert(node.id);
        }
    }

    pub fn index_node_props(&mut self, node: &StoredNode) {
        for label in &node.labels {
            for (key, value) in &node.props {
                if let Some(value_key) = ValueKey::from_value(value) {
                    self.node_props
                        .entry((label.clone(), key.clone(), value_key))
                        .or_default()
                        .insert(node.id);
                }
            }
        }
    }

    fn deindex_node(&mut self, node: &StoredNode) {
        for label in &node.labels {
            remove_index_entry(&mut self.nodes_by_label, label, node.id);
            for (key, value) in &node.props {
                if let Some(value_key) = ValueKey::from_value(value) {
                    remove_index_entry(
                        &mut self.node_props,
                        &(label.clone(), key.clone(), value_key),
                        node.id,
                    );
                }
            }
        }
    }

    fn index_relationship(&mut self, rel: &StoredRel) {
        self.rels_by_type
            .entry(rel.rel_type.clone())
            .or_default()
            .insert(rel.id);
        self.outgoing
            .entry((rel.from, rel.rel_type.clone()))
            .or_default()
            .insert(rel.id);
        self.incoming
            .entry((rel.to, rel.rel_type.clone()))
            .or_default()
            .insert(rel.id);
        self.outgoing_any
            .entry(rel.from)
            .or_default()
            .insert(rel.id);
        self.incoming_any.entry(rel.to).or_default().insert(rel.id);
    }

    fn deindex_relationship(&mut self, rel: &StoredRel) {
        remove_index_entry(&mut self.rels_by_type, &rel.rel_type, rel.id);
        remove_index_entry(
            &mut self.outgoing,
            &(rel.from, rel.rel_type.clone()),
            rel.id,
        );
        remove_index_entry(&mut self.incoming, &(rel.to, rel.rel_type.clone()), rel.id);
        remove_index_entry(&mut self.outgoing_any, &rel.from, rel.id);
        remove_index_entry(&mut self.incoming_any, &rel.to, rel.id);
    }
}

fn remove_index_entry<K: Ord + Clone>(index: &mut BTreeMap<K, BTreeSet<i64>>, key: &K, id: i64) {
    let should_remove = if let Some(ids) = index.get_mut(key) {
        ids.remove(&id);
        ids.is_empty()
    } else {
        false
    };
    if should_remove {
        index.remove(key);
    }
}
