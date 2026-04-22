use std::collections::BTreeMap;

use crate::{GraphQuery, GrmError, StoredNode, StoredRel, dsl::QueryResult, error::Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendIdType {
    Int64,
    Uuid,
}

impl BackendIdType {
    pub fn keyword(&self) -> &'static str {
        match self {
            Self::Int64 => "int",
            Self::Uuid => "uuid",
        }
    }
}

#[async_trait]
pub trait GraphTx {
    async fn execute_query(
        &mut self,
        _query: &str,
        _params: Value
    ) -> Result<QueryResult> {
        Err(GrmError::NotSupported("execute_query"))
    }

    async fn execute_graph(
        &mut self, _q: &GraphQuery
    ) -> Result<QueryResult>;

    async fn create_node(
        &mut self,
        _labels: Vec<String>,
        _props: BTreeMap<String, Value>,
    ) -> Result<StoredNode> {
        Err(GrmError::NotSupported("create_node"))
    }

    async fn update_node(
        &mut self,
        _id: i64,
        _props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        Err(GrmError::NotSupported("update_node"))
    }

    async fn delete_node(&mut self, _id: i64) -> Result<()> {
        Err(GrmError::NotSupported("delete_node"))
    }

    async fn find_node_by_id(&mut self, _id: i64) -> Result<Option<StoredNode>> {
        Err(GrmError::NotSupported("find_node_by_id"))
    }

    async fn find_nodes_by_property(
        &mut self,
        _key: &str,
        _value: &Value,
    ) -> Result<Vec<StoredNode>> {
        Err(GrmError::NotSupported("find_nodes_by_property"))
    }

    async fn create_relationship(
        &mut self,
        _from: i64,
        _to: i64,
        _rel_type: &str,
        _props: BTreeMap<String, Value>,
    ) -> Result<StoredRel> {
        Err(GrmError::NotSupported("create_relationship"))
    }

    async fn update_relationship(
        &mut self,
        _id: i64,
        _props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredRel>> {
        Err(GrmError::NotSupported("update_relationship"))
    }

    async fn delete_relationship(&mut self, _id: i64) -> Result<()> {
        Err(GrmError::NotSupported("delete_relationship"))
    }

    async fn outgoing(
        &mut self,
        _from: i64,
        _rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Err(GrmError::NotSupported("outgoing"))
    }

    async fn incoming(
        &mut self,
        _to: i64,
        _rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Err(GrmError::NotSupported("incoming"))
    }

    async fn both(
        &mut self,
        _node: i64,
        _rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        Err(GrmError::NotSupported("both"))
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

pub trait BackendIdentity: GraphBackend {
    fn node_id_type(&self) -> BackendIdType;

    fn rel_id_type(&self) -> BackendIdType {
        self.node_id_type()
    }
}
