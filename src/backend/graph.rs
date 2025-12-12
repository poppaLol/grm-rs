use async_trait::async_trait;
use serde_json::Value;
use crate::error::Result;

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
}
