use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;

use crate::backend::inmemory::InMemoryTx;
use crate::backend::{GraphTx, StoredNode, StoredRel};
use crate::dsl::{GraphQuery, QueryResult};
use crate::error::{GrmError, Result};

#[async_trait]
impl GraphTx for InMemoryTx {
    async fn execute_query(&mut self, _query: &str, _params: Value) -> Result<QueryResult> {
        Err(GrmError::Backend(
            "InMemory backend does not support string queries; use typed APIs".into(),
        ))
    }

    async fn execute_graph(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        self.execute_graph_query(q).await
    }

    async fn create_node(
        &mut self,
        labels: Vec<String>,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredNode> {
        let id = self.working_copy.next_node_id;
        self.working_copy.next_node_id += 1;

        let node = StoredNode { id, labels, props };
        self.working_copy.nodes.insert(id, node.clone());
        Ok(node)
    }

    async fn update_node(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        if let Some(node) = self.working_copy.nodes.get_mut(&id) {
            for (k, v) in props {
                node.props.insert(k, v);
            }
            return Ok(Some(node.clone()));
        }
        Ok(None)
    }

    async fn delete_node(&mut self, id: i64) -> Result<()> {
        self.working_copy.nodes.remove(&id);
        self.working_copy
            .rels
            .retain(|_, rel| rel.from != id && rel.to != id);
        Ok(())
    }

    async fn find_node_by_id(&mut self, id: i64) -> Result<Option<StoredNode>> {
        Ok(self.working_copy.nodes.get(&id).cloned())
    }

    async fn find_nodes_by_property(
        &mut self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<StoredNode>> {
        Ok(self
            .working_copy
            .nodes
            .values()
            .filter(|n| n.props.get(key).map(|v| v == value).unwrap_or(false))
            .cloned()
            .collect())
    }

    async fn create_relationship(
        &mut self,
        from: i64,
        to: i64,
        rel_type: &str,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredRel> {
        let id = self.working_copy.next_rel_id;
        self.working_copy.next_rel_id += 1;

        let rel = StoredRel {
            id,
            rel_type: rel_type.to_string(),
            from,
            to,
            props,
        };
        self.working_copy.rels.insert(id, rel.clone());
        Ok(rel)
    }

    async fn outgoing(
        &mut self,
        from: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let mut out = Vec::new();
        for rel in self.working_copy.rels.values() {
            if rel.from != from {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.to) {
                out.push((rel.clone(), n.clone()));
            }
        }
        Ok(out)
    }

    async fn incoming(
        &mut self,
        to: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let mut out = Vec::new();
        for rel in self.working_copy.rels.values() {
            if rel.to != to {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.from) {
                out.push((rel.clone(), n.clone()));
            }
        }
        Ok(out)
    }

    async fn both(
        &mut self,
        node: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let mut out = Vec::new();
        let mut seen_rel_ids = std::collections::BTreeSet::new();

        // outgoing neighbors
        for rel in self.working_copy.rels.values() {
            if rel.from != node {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.to) {
                if seen_rel_ids.insert(rel.id) {
                    out.push((rel.clone(), n.clone()));
                }
            }
        }

        // incoming neighbors
        for rel in self.working_copy.rels.values() {
            if rel.to != node {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.from) {
                if seen_rel_ids.insert(rel.id) {
                    out.push((rel.clone(), n.clone()));
                }
            }
        }

        Ok(out)
    }

    async fn commit(mut self) -> Result<()> {
        let mut global = self.store.lock().unwrap();
        *global = self.working_copy.clone();
        self.committed = true;
        Ok(())
    }

    async fn rollback(mut self) -> Result<()> {
        self.committed = true;
        Ok(())
    }
}