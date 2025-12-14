use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use grm_rs::{GraphBackend, GraphTx, InMemoryBackend, NodeModel, QueryResult, RelModel, Result, StoredNode, StoredRel, typed_id};

/*
 * This file contains some sample entities we can use for testing the codebase
 * In each case there is a strongly
 * e.g. UserId / User. Additionally you should be able to see properties for the fields e.g. name_prop being
 * the reference for name property "title"
 */
use serde::{Deserialize, Serialize};
use serde_json::Value;

typed_id!(UserId);
typed_id!(PostId);
typed_id!(AuthoredId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct User {
    #[grm(id)]
    #[serde(skip)]
    pub(crate) id: UserId,
    pub name: String,
    pub age: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct Post {
    #[grm(id)]
    #[serde(skip)]
    pub id: PostId,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "User", to = "Post", ty = "AUTHORED")]
pub struct Authored {
    #[grm(id)]
    #[serde(skip)]
    pub id: AuthoredId,
    pub year: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct A {
    #[grm(id)]
    #[serde(skip)]
    id: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct B {
    #[grm(id)]
    #[serde(skip)]
    id: i64,
    // required property so decode can fail
    must_have: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "A", to = "B", ty = "AB")]
pub struct AB {
    #[grm(id)]
    #[serde(skip)]
    id: i64,
}

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

    async fn commit(self) -> grm_rs::error::Result<()> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        self.inner.commit().await
    }

    async fn rollback(self) -> grm_rs::error::Result<()> {
        self.inner.rollback().await
    }

    async fn create_node(
        &mut self,
        labels: Vec<String>,
        props: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> grm_rs::error::Result<StoredNode> {
        self.inner.create_node(labels, props).await
    }

    async fn create_relationship(
        &mut self,
        from: i64,
        to: i64,
        rel_type: String,
        props: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> grm_rs::error::Result<StoredRel> {
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

    async fn execute_query(
        &mut self,
        query: &str,
        params: serde_json::Value,
    ) -> grm_rs::error::Result<grm_rs::backend::QueryResult> {
        self.inner.execute_query(query, params).await
    }
}
