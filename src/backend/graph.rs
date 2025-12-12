use async_trait::async_trait;
use serde_json::Value;
use crate::{GrmError, dsl::GraphQuery, error::Result};

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
    async fn execute_query(
        &mut self,
        query: &str,
        params: Value,
    ) -> Result<QueryResult>;

    async fn execute_graph(&mut self, _q: &GraphQuery) -> Result<QueryResult> {
        Err(GrmError::Backend("execute_graph not supported by this backend".into()))
    }

    async fn commit(self) -> Result<()>;
    async fn rollback(self) -> Result<()>;
}

#[async_trait]
pub trait GraphBackend: Send + Sync + Clone {
    type Tx: GraphTx + Send;

    async fn execute_query(
        &self,
        query: &str,
        params: Value,
    ) -> Result<QueryResult>;

    async fn begin_tx(&self) -> Result<Self::Tx>;

    async fn execute_graph(&self, q: &GraphQuery) -> Result<QueryResult> {
        let mut tx = self.begin_tx().await?;
        let out = tx.execute_graph(q).await?;
        tx.commit().await?;
        Ok(out)
    }
}
