use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

// Import public types from dsl
use crate::dsl::{KernelNodeId, KernelRelId};
use crate::error::Result;
use crate::fsutil::{backup_path, write_file_atomically_with_backup};

// Use the public types from the backend module
use crate::backend::{GraphStore, StoredNode, StoredRel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedGraphStore {
    pub next_node_id: KernelNodeId,
    pub next_rel_id: KernelRelId,
    pub nodes: BTreeMap<KernelNodeId, StoredNode>,
    pub rels: BTreeMap<KernelRelId, StoredRel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BinaryStoredNode {
    id: KernelNodeId,
    labels: Vec<String>,
    props: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BinaryStoredRel {
    id: KernelRelId,
    rel_type: String,
    from: KernelNodeId,
    to: KernelNodeId,
    props: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BinaryPersistedGraphStore {
    next_node_id: KernelNodeId,
    next_rel_id: KernelRelId,
    nodes: BTreeMap<KernelNodeId, BinaryStoredNode>,
    rels: BTreeMap<KernelRelId, BinaryStoredRel>,
}

fn encode_props(props: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Vec<u8>>> {
    props
        .iter()
        .map(|(key, value)| {
            serde_json::to_vec(value)
                .map(|bytes| (key.clone(), bytes))
                .map_err(|_| crate::error::GrmError::SaveAborted("failed to encode property value"))
        })
        .collect()
}

fn decode_props(props: BTreeMap<String, Vec<u8>>) -> Result<BTreeMap<String, Value>> {
    props
        .into_iter()
        .map(|(key, bytes)| {
            serde_json::from_slice(&bytes)
                .map(|value| (key, value))
                .map_err(|_| crate::error::GrmError::LoadAborted("failed to decode property value"))
        })
        .collect()
}

impl GraphStore {
    pub fn to_persisted(&self) -> PersistedGraphStore {
        PersistedGraphStore {
            next_node_id: self.next_node_id,
            next_rel_id: self.next_rel_id,
            nodes: self.nodes.clone(),
            rels: self.rels.clone(),
        }
    }

    pub fn from_persisted(persisted: PersistedGraphStore) -> Self {
        let mut store = Self {
            next_node_id: persisted.next_node_id,
            next_rel_id: persisted.next_rel_id,
            nodes: persisted.nodes,
            rels: persisted.rels,
            nodes_by_label: BTreeMap::new(),
            node_props: BTreeMap::new(),
            node_props_any: BTreeMap::new(),
            node_props_dirty: false,
            rels_by_type: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing_any: BTreeMap::new(),
            incoming_any: BTreeMap::new(),
        };
        store.rebuild_load_indexes();
        store
    }

    pub(crate) fn to_binary_persisted(&self) -> Result<BinaryPersistedGraphStore> {
        let nodes = self
            .nodes
            .iter()
            .map(|(id, node)| {
                Ok((
                    *id,
                    BinaryStoredNode {
                        id: node.id,
                        labels: node.labels.clone(),
                        props: encode_props(&node.props)?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        let rels = self
            .rels
            .iter()
            .map(|(id, rel)| {
                Ok((
                    *id,
                    BinaryStoredRel {
                        id: rel.id,
                        rel_type: rel.rel_type.clone(),
                        from: rel.from,
                        to: rel.to,
                        props: encode_props(&rel.props)?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        Ok(BinaryPersistedGraphStore {
            next_node_id: self.next_node_id,
            next_rel_id: self.next_rel_id,
            nodes,
            rels,
        })
    }

    pub(crate) fn from_binary_persisted(persisted: BinaryPersistedGraphStore) -> Result<Self> {
        let nodes = persisted
            .nodes
            .into_iter()
            .map(|(id, node)| {
                Ok((
                    id,
                    StoredNode {
                        id: node.id,
                        labels: node.labels,
                        props: decode_props(node.props)?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        let rels = persisted
            .rels
            .into_iter()
            .map(|(id, rel)| {
                Ok((
                    id,
                    StoredRel {
                        id: rel.id,
                        rel_type: rel.rel_type,
                        from: rel.from,
                        to: rel.to,
                        props: decode_props(rel.props)?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        let mut store = Self {
            next_node_id: persisted.next_node_id,
            next_rel_id: persisted.next_rel_id,
            nodes,
            rels,
            nodes_by_label: BTreeMap::new(),
            node_props: BTreeMap::new(),
            node_props_any: BTreeMap::new(),
            node_props_dirty: false,
            rels_by_type: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing_any: BTreeMap::new(),
            incoming_any: BTreeMap::new(),
        };
        store.rebuild_load_indexes();
        Ok(store)
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.to_persisted()).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to serialize graph as JSON")
        })?;
        write_file_atomically_with_backup(path, json.as_bytes())
            .map_err(|_| crate::error::GrmError::SaveAborted("failed to write JSON graph file"))?;
        Ok(())
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = fs::read_to_string(path)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to read JSON graph file"))?;
        match serde_json::from_str::<PersistedGraphStore>(&json) {
            Ok(persisted) => Ok(Self::from_persisted(persisted)),
            Err(_) => {
                let backup = backup_path(path);
                let json = fs::read_to_string(backup).map_err(|_| {
                    crate::error::GrmError::LoadAborted("failed to deserialize JSON graph file")
                })?;
                let persisted: PersistedGraphStore = serde_json::from_str(&json).map_err(|_| {
                    crate::error::GrmError::LoadAborted("failed to deserialize JSON graph file")
                })?;
                Ok(Self::from_persisted(persisted))
            }
        }
    }

    pub fn save_to_binary_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = bincode::serialize(&self.to_binary_persisted()?).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to serialize graph as binary")
        })?;
        write_file_atomically_with_backup(path, &bytes).map_err(|_| {
            crate::error::GrmError::SaveAborted("failed to write binary graph file")
        })?;
        Ok(())
    }

    pub fn load_from_binary_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path)
            .map_err(|_| crate::error::GrmError::LoadAborted("failed to read binary graph file"))?;
        match bincode::deserialize::<BinaryPersistedGraphStore>(&bytes) {
            Ok(persisted) => Self::from_binary_persisted(persisted),
            Err(_) => {
                let backup = backup_path(path);
                let bytes = fs::read(backup).map_err(|_| {
                    crate::error::GrmError::LoadAborted("failed to deserialize binary graph file")
                })?;
                let persisted: BinaryPersistedGraphStore =
                    bincode::deserialize(&bytes).map_err(|_| {
                        crate::error::GrmError::LoadAborted(
                            "failed to deserialize binary graph file",
                        )
                    })?;
                Self::from_binary_persisted(persisted)
            }
        }
    }
}

// Implement the GraphPersistence trait for GraphStore
impl crate::backend::GraphPersistence for GraphStore {
    fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        GraphStore::save_to_file(self, path)
    }

    fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        GraphStore::load_from_file(path)
    }

    fn save_to_binary_file(&self, path: impl AsRef<Path>) -> Result<()> {
        GraphStore::save_to_binary_file(self, path)
    }

    fn load_from_binary_file(path: impl AsRef<Path>) -> Result<Self> {
        GraphStore::load_from_binary_file(path)
    }
}
