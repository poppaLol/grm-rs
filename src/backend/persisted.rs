use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

// Import public types from dsl
use crate::dsl::{KernelNodeId, KernelRelId};
use crate::error::Result;

// Use the public types from the backend module
use crate::backend::{StoredNode, StoredRel, GraphStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedGraphStore {
    pub next_node_id: KernelNodeId,
    pub next_rel_id: KernelRelId,
    pub nodes: BTreeMap<KernelNodeId, StoredNode>,
    pub rels: BTreeMap<KernelRelId, StoredRel>,
}

impl GraphStore {
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let persisted = PersistedGraphStore {
            next_node_id: self.next_node_id,
            next_rel_id: self.next_rel_id,
            nodes: self.nodes.clone(),
            rels: self.rels.clone(),
        };
        let json = serde_json::to_string_pretty(&persisted).expect("Serde Error");
        fs::write(path, json).expect("File system error");
        Ok(())
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let json = fs::read_to_string(path).expect("File system error");
        let persisted: PersistedGraphStore = serde_json::from_str(&json).expect("Serde Error");
        Ok(Self {
            next_node_id: persisted.next_node_id,
            next_rel_id: persisted.next_rel_id,
            nodes: persisted.nodes,
            rels: persisted.rels,
        })
    }
}

// Implement the GraphPersistence trait for GraphStore
impl crate::backend::GraphPersistence for GraphStore {
    fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let persisted = PersistedGraphStore {
            next_node_id: self.next_node_id,
            next_rel_id: self.next_rel_id,
            nodes: self.nodes.clone(),
            rels: self.rels.clone(),
        };
        let json = serde_json::to_string_pretty(&persisted).expect("Serde Error");
        fs::write(path, json).expect("File system error");
        Ok(())
    }

    fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let json = fs::read_to_string(path).expect("File system error");
        let persisted: PersistedGraphStore = serde_json::from_str(&json).expect("Serde Error");
        Ok(Self {
            next_node_id: persisted.next_node_id,
            next_rel_id: persisted.next_rel_id,
            nodes: persisted.nodes,
            rels: persisted.rels,
        })
    }
}