use std::collections::{BTreeMap, BTreeSet};

use crate::backend::{StoredNode, StoredRel};

#[derive(Debug, Clone)]
pub struct GraphStore {
    pub next_node_id: i64,
    pub next_rel_id: i64,
    pub nodes: BTreeMap<i64, StoredNode>,
    pub rels: BTreeMap<i64, StoredRel>,
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
            outgoing: self.outgoing.clone(),
            incoming: self.incoming.clone(),
            outgoing_any: self.outgoing_any.clone(),
            incoming_any: self.incoming_any.clone(),
        }
    }

    pub fn rebuild_indexes(&mut self) {
        self.outgoing.clear();
        self.incoming.clear();
        self.outgoing_any.clear();
        self.incoming_any.clear();

        for rel in self.rels.values().cloned().collect::<Vec<_>>() {
            self.index_relationship(&rel);
        }
    }

    pub fn remove_node(&mut self, id: i64) -> Option<StoredNode> {
        let removed = self.nodes.remove(&id);
        if removed.is_some() {
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

    fn index_relationship(&mut self, rel: &StoredRel) {
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
