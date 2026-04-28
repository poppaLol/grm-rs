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
        self.working_copy.insert_node(id, node.clone());
        Ok(node)
    }

    async fn update_node(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        if let Some(mut node) = self.working_copy.nodes.get(&id).cloned() {
            for (k, v) in props {
                node.props.insert(k, v);
            }
            self.working_copy.insert_node(id, node.clone());
            return Ok(Some(node.clone()));
        }
        Ok(None)
    }

    async fn delete_node(&mut self, id: i64) -> Result<()> {
        self.working_copy.remove_node(id);
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
        self.working_copy.insert_relationship(id, rel.clone());
        Ok(rel)
    }

    async fn update_relationship(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredRel>> {
        if let Some(rel) = self.working_copy.rels.get_mut(&id) {
            for (k, v) in props {
                rel.props.insert(k, v);
            }
            return Ok(Some(rel.clone()));
        }
        Ok(None)
    }

    async fn delete_relationship(&mut self, id: i64) -> Result<()> {
        self.working_copy.remove_relationship(id);
        Ok(())
    }

    async fn outgoing(
        &mut self,
        from: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let rel_ids = self.working_copy.outgoing_relationship_ids(from, rel_type);
        let mut out = Vec::with_capacity(rel_ids.len());
        for rel_id in rel_ids {
            let Some(rel) = self.working_copy.rels.get(&rel_id) else {
                continue;
            };
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
        let rel_ids = self.working_copy.incoming_relationship_ids(to, rel_type);
        let mut out = Vec::with_capacity(rel_ids.len());
        for rel_id in rel_ids {
            let Some(rel) = self.working_copy.rels.get(&rel_id) else {
                continue;
            };
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

        for rel_id in self.working_copy.outgoing_relationship_ids(node, rel_type) {
            let Some(rel) = self.working_copy.rels.get(&rel_id) else {
                continue;
            };
            if let Some(n) = self.working_copy.nodes.get(&rel.to) {
                if seen_rel_ids.insert(rel.id) {
                    out.push((rel.clone(), n.clone()));
                }
            }
        }

        for rel_id in self.working_copy.incoming_relationship_ids(node, rel_type) {
            let Some(rel) = self.working_copy.rels.get(&rel_id) else {
                continue;
            };
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
        *global = self.working_copy;
        self.committed = true;
        Ok(())
    }

    async fn rollback(mut self) -> Result<()> {
        self.committed = true;
        Ok(())
    }
}
