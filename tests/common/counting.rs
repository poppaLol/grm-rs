use std::{collections::BTreeMap, sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
}};
use grm_rs::{GraphBackend, GraphQuery, GraphTx, InMemoryBackend, QueryResult, Result, StoredNode, StoredRel};
use serde_json::Value;



#[derive(Clone)]
pub struct CountingBackend {
    pub(crate) inner: InMemoryBackend,
    pub(crate) commits: Arc<AtomicUsize>,
}

pub struct CountingTx<T> {
    pub(crate) inner: T,
    pub(crate) commits: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl GraphBackend for CountingBackend {
    type Tx = CountingTx<<InMemoryBackend as GraphBackend>::Tx>;

    async fn begin_tx(&self) -> Result<Self::Tx> {
        let tx = self.inner.begin_tx().await?;
        Ok(CountingTx { inner: tx, commits: self.commits.clone() })
    }

    async fn execute_query(
        &self,
        query: &str,
        params: Value,
    ) -> Result<QueryResult> {
        self.inner.execute_query(query, params).await
    }
}

#[async_trait::async_trait]
impl<T: GraphTx + Send> GraphTx for CountingTx<T> {
    // Delegate everything you use in this test:

    async fn commit(self) -> Result<()> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        self.inner.commit().await
    }

    async fn rollback(self) -> Result<()> {
        self.inner.rollback().await
    }

    async fn create_node(
        &mut self,
        labels: Vec<String>,
        props: BTreeMap<String, serde_json::Value>,
    ) -> Result<StoredNode> {
        self.inner.create_node(labels, props).await
    }

    async fn create_relationship(
        &mut self,
        from: i64,
        to: i64,
        rel_type: &str,
        props: BTreeMap<String, serde_json::Value>,
    ) -> Result<StoredRel> {
        self.inner.create_relationship(from, to, rel_type, props).await
    }

    async fn outgoing(&mut self, from: i64, rel_type: Option<&str>) -> Result<Vec<(StoredRel, StoredNode)>> {
        self.inner.outgoing(from, rel_type).await
    }

    // If your trait now has these, delegate as well (harmless if unused):
    async fn incoming(&mut self, to: i64, rel_type: Option<&str>) -> Result<Vec<(StoredRel, StoredNode)>> {
        self.inner.incoming(to, rel_type).await
    }

    async fn both(&mut self, node: i64, rel_type: Option<&str>) -> Result<Vec<(StoredRel, StoredNode)>> {
        self.inner.both(node, rel_type).await
    }

    async fn execute_graph(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        self.inner.execute_graph(q).await
    }
}
