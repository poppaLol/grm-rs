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
        if self.working_copy.is_some() {
            let id = self.allocate_node_id();
            let node = StoredNode { id, labels, props };
            let store = self.materialized_store_mut();
            store.next_node_id = store.next_node_id.max(id + 1);
            store.insert_node(id, node.clone());
            self.delta.deleted_nodes.remove(&id);
            self.delta.nodes.insert(id, node.clone());
            return Ok(node);
        }

        let id = self.allocate_node_id();
        let node = StoredNode { id, labels, props };
        self.delta.deleted_nodes.remove(&id);
        self.delta.nodes.insert(id, node.clone());
        Ok(node)
    }

    async fn create_nodes(
        &mut self,
        inserts: Vec<(Vec<String>, BTreeMap<String, Value>)>,
    ) -> Result<Vec<StoredNode>> {
        let mut nodes = Vec::with_capacity(inserts.len());
        for (labels, props) in inserts {
            let id = self.allocate_node_id();

            let node = StoredNode { id, labels, props };
            if self.working_copy.is_some() {
                let store = self.materialized_store_mut();
                store.next_node_id = store.next_node_id.max(id + 1);
                store.insert_node_deferred_property_index(id, node.clone());
                self.delta.deleted_nodes.remove(&id);
                self.delta.nodes.insert(id, node.clone());
            } else {
                self.delta.deleted_nodes.remove(&id);
                self.delta.nodes.insert(id, node.clone());
            }
            nodes.push(node);
        }
        Ok(nodes)
    }

    async fn update_node(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        if self.working_copy.is_some() {
            if let Some(mut node) = self.materialized_store().nodes.get(&id).cloned() {
                for (k, v) in props {
                    node.props.insert(k, v);
                }
                self.materialized_store_mut().insert_node(id, node.clone());
                self.delta.deleted_nodes.remove(&id);
                self.delta.nodes.insert(id, node.clone());
                return Ok(Some(node.clone()));
            }
            return Ok(None);
        }

        if self.delta.deleted_nodes.contains(&id) {
            return Ok(None);
        }

        let existing = self
            .delta
            .nodes
            .get(&id)
            .cloned()
            .or_else(|| self.store.lock().unwrap().nodes.get(&id).cloned());

        if let Some(mut node) = existing {
            for (k, v) in props {
                node.props.insert(k, v);
            }
            self.delta.nodes.insert(id, node.clone());
            return Ok(Some(node.clone()));
        }
        Ok(None)
    }

    async fn delete_node(&mut self, id: i64) -> Result<()> {
        let related_rel_ids = self.visible_related_rel_ids(id);

        if self.working_copy.is_some() {
            self.materialized_store_mut().remove_node(id);
        }
        if self.delta.nodes.remove(&id).is_none() {
            self.delta.deleted_nodes.insert(id);
        }
        for rel_id in related_rel_ids {
            if self.delta.rels.remove(&rel_id).is_none() {
                self.delta.deleted_rels.insert(rel_id);
            }
        }
        Ok(())
    }

    async fn find_node_by_id(&mut self, id: i64) -> Result<Option<StoredNode>> {
        if self.working_copy.is_some() {
            return Ok(self.materialized_store().nodes.get(&id).cloned());
        }
        if self.delta.deleted_nodes.contains(&id) {
            return Ok(None);
        }
        Ok(self
            .delta
            .nodes
            .get(&id)
            .cloned()
            .or_else(|| self.store.lock().unwrap().nodes.get(&id).cloned()))
    }

    async fn find_nodes_by_property(
        &mut self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<StoredNode>> {
        Ok(self.visible_nodes_by_property(key, value))
    }

    async fn create_relationship(
        &mut self,
        from: i64,
        to: i64,
        rel_type: &str,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredRel> {
        let id = self.allocate_rel_id();

        let rel = StoredRel {
            id,
            rel_type: rel_type.to_string(),
            from,
            to,
            props,
        };
        if self.working_copy.is_some() {
            let store = self.materialized_store_mut();
            store.next_rel_id = store.next_rel_id.max(id + 1);
            store.insert_relationship(id, rel.clone());
            self.delta.deleted_rels.remove(&id);
            self.delta.rels.insert(id, rel.clone());
        } else {
            self.delta.deleted_rels.remove(&id);
            self.delta.rels.insert(id, rel.clone());
        }
        Ok(rel)
    }

    async fn update_relationship(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredRel>> {
        if self.working_copy.is_some() {
            if let Some(rel) = self.materialized_store_mut().rels.get_mut(&id) {
                for (k, v) in props {
                    rel.props.insert(k, v);
                }
                let rel = rel.clone();
                self.delta.deleted_rels.remove(&id);
                self.delta.rels.insert(id, rel.clone());
                return Ok(Some(rel.clone()));
            }
            return Ok(None);
        }

        if self.delta.deleted_rels.contains(&id) {
            return Ok(None);
        }

        let existing = self
            .delta
            .rels
            .get(&id)
            .cloned()
            .or_else(|| self.store.lock().unwrap().rels.get(&id).cloned());

        if let Some(mut rel) = existing {
            for (k, v) in props {
                rel.props.insert(k, v);
            }
            self.delta.rels.insert(id, rel.clone());
            return Ok(Some(rel.clone()));
        }
        Ok(None)
    }

    async fn delete_relationship(&mut self, id: i64) -> Result<()> {
        if self.working_copy.is_some() {
            self.materialized_store_mut().remove_relationship(id);
        }
        if self.delta.rels.remove(&id).is_none() {
            self.delta.deleted_rels.insert(id);
        }
        Ok(())
    }

    async fn outgoing(
        &mut self,
        from: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Ok(self.visible_neighbor_pairs(from, rel_type, crate::dsl::Direction::Out))
    }

    async fn incoming(
        &mut self,
        to: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Ok(self.visible_neighbor_pairs(to, rel_type, crate::dsl::Direction::In))
    }

    async fn both(
        &mut self,
        node: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Ok(self.visible_neighbor_pairs(node, rel_type, crate::dsl::Direction::Both))
    }

    async fn commit(mut self) -> Result<()> {
        let mut global = self.store.lock().unwrap();
        self.apply_delta_to(&mut global);
        self.committed = true;
        Ok(())
    }

    async fn rollback(mut self) -> Result<()> {
        self.committed = true;
        Ok(())
    }
}
