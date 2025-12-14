use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;
use crate::{GrmError, backend::{StoredNode, StoredRel}, dsl::GraphQuery, error::Result};

#[derive(Debug, Clone)]
pub struct QueryRow {
    pub values: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub rows: Vec<QueryRow>,
}

#[async_trait]
pub trait GraphTx {
    async fn execute_query(&mut self, query: &str, params: Value) -> Result<QueryResult>;

    async fn execute_graph(&mut self, _q: &GraphQuery) -> Result<QueryResult> {
        Err(GrmError::Backend("execute_graph not supported".into()))
    }

    async fn create_node(
        &mut self,
        _labels: Vec<String>,
        _props: BTreeMap<String, Value>,
    ) -> Result<StoredNode> {
        Err(GrmError::Backend("create_node not supported".into()))
    }

    async fn update_node(
        &mut self,
        _id: i64,
        _props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        Err(GrmError::Backend("update_node not supported".into()))
    }

    async fn delete_node(&mut self, _id: i64) -> Result<()> {
        Err(GrmError::Backend("delete_node not supported".into()))
    }

    async fn find_node_by_id(&mut self, _id: i64) -> Result<Option<StoredNode>> {
        Err(GrmError::Backend("find_node_by_id not supported".into()))
    }

    async fn find_nodes_by_property(
        &mut self,
        _key: &str,
        _value: &Value,
    ) -> Result<Vec<StoredNode>> {
        Err(GrmError::Backend("find_nodes_by_property not supported".into()))
    }

    async fn create_relationship(
        &mut self,
        _from: i64,
        _to: i64,
        _rel_type: String,
        _props: BTreeMap<String, Value>,
    ) -> Result<StoredRel> {
        Err(GrmError::Backend("create_relationship not supported".into()))
    }

    async fn outgoing(
        &mut self,
        _from: i64,
        _rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Err(GrmError::Backend("outgoing not supported".into()))
    }

    async fn incoming(
        &mut self,
        _to: i64,
        _rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Err(GrmError::Backend("incoming not supported".into()))
    }

    async fn both(
        &mut self,
        _node: i64,
        _rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Err(GrmError::Backend("both not supported".into()))
    }

    async fn commit(self) -> Result<()>;
    async fn rollback(self) -> Result<()>;
}

#[async_trait]
pub trait GraphBackend: Send + Sync + Clone {
    type Tx: GraphTx + Send;

    async fn execute_query(&self, query: &str, params: Value) -> Result<QueryResult>;
    async fn begin_tx(&self) -> Result<Self::Tx>;

    async fn execute_graph(&self, q: &GraphQuery) -> Result<QueryResult> {
        let mut tx = self.begin_tx().await?;
        let out = tx.execute_graph(q).await?;
        tx.commit().await?;
        Ok(out)
    }
}