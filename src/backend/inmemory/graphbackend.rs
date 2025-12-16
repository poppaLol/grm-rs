use crate::{
    GraphBackend, GraphQuery, GrmError, InMemoryBackend, QueryResult, backend::inmemory::inmemorytx::InMemoryTx
};
use async_trait::async_trait;
use serde_json::Value;
use crate::error::{Result};

#[async_trait]
impl GraphBackend for InMemoryBackend {
    type Tx = InMemoryTx;

    async fn execute_query(&self, _query: &str, _params: Value) -> Result<QueryResult> {
        Err(GrmError::Backend(
            "InMemoryBackend does not support string queries; use execute_graph (typed)".into(),
        ))
    }

    async fn begin_tx(&self) -> Result<Self::Tx> {
        Ok(InMemoryTx::new(self.store.clone()))
    }

    // Optional: implement directly (otherwise the trait default uses begin_tx + commit)
    async fn execute_graph(&self, q: &GraphQuery) -> Result<QueryResult> {
        let mut tx = InMemoryTx::new(self.store.clone());
        tx.execute_graph_query(q).await
    }
}
