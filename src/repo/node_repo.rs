use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::client::{QueryExecution, Transaction};
use serde_json::Value as JsonValue;

use crate::backend::{GraphBackend, GraphTx};
use crate::dsl::Query;
use crate::error::Result;
use crate::model::NodeModel;
use crate::{DecodeFromRow, GraphClient};

/// Decode a StoredNode into M.
fn decode_stored_node<M: NodeModel>(id: i64, props: BTreeMap<String, JsonValue>) -> Result<M> {
    M::from_properties(id.into(), props)
}

pub struct NodeRepository<B, M>
where
    B: GraphBackend,
    M: NodeModel,
{
    backend: B,
    _marker: PhantomData<M>,
}

impl<B, M> NodeRepository<B, M>
where
    B: GraphBackend + Clone,
    M: NodeModel,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            _marker: PhantomData,
        }
    }

    pub async fn execute<R: NodeModel>(&self, q: Query<R>) -> Result<QueryExecution> {
        let client = GraphClient::new(self.backend.clone());
        let mut tx = client.transaction().await?;
        let exec = tx.execute(q).await?;
        tx.commit().await?;
        Ok(exec)
    }

    pub async fn fetch<R: NodeModel>(&self, q: Query<R>) -> Result<Vec<M>>
    where
        M: DecodeFromRow,
    {
        let exec = self.execute(q).await?;
        exec.decode_all::<M>()
    }

    /// Create a node using typed tx CRUD.
    pub async fn create(&self, model: &mut M) -> Result<()> {
        let mut tx = self.backend.begin_tx().await?;

        let labels = M::LABELS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let props: BTreeMap<String, JsonValue> = model.to_properties();

        let stored = tx.create_node(labels, props).await?;
        tx.commit().await?;

        model.set_id(stored.id.into());
        Ok(())
    }

    /// Find a node by internal id using typed tx CRUD.
    pub async fn find_by_id(&self, id: &M::Id) -> Result<Option<M>> {
        let raw_id: i64 = id.clone().into();

        let mut tx = self.backend.begin_tx().await?;
        let stored_opt = tx.find_node_by_id(raw_id).await?;
        tx.commit().await?;

        match stored_opt {
            Some(stored) => {
                // Optional defensive label check (recommended)
                // If you don’t want this, remove it.
                let ok = M::LABELS
                    .iter()
                    .all(|l| stored.labels.iter().any(|sl| sl == l));
                if !ok {
                    return Ok(None);
                }

                Ok(Some(decode_stored_node::<M>(stored.id, stored.props)?))
            }
            None => Ok(None),
        }
    }

    /// Find nodes by a single property equality using typed tx CRUD.
    ///
    /// Note: returns all matches; not unique.
    pub async fn find_by(&self, key: &str, value: &JsonValue) -> Result<Vec<M>> {
        let mut tx = self.backend.begin_tx().await?;
        let stored = tx.find_nodes_by_property(key, value).await?;
        tx.commit().await?;

        let mut out = Vec::with_capacity(stored.len());
        for n in stored {
            // Optional defensive label check
            let ok = M::LABELS.iter().all(|l| n.labels.iter().any(|sl| sl == l));
            if !ok {
                continue;
            }
            out.push(decode_stored_node::<M>(n.id, n.props)?);
        }
        Ok(out)
    }

    /// Update node properties (SET += semantics) using typed tx CRUD.
    pub async fn update(&self, model: &M) -> Result<()> {
        let raw_id: i64 = model.id().clone().into();
        let props: BTreeMap<String, JsonValue> = model.to_properties();

        let mut tx = self.backend.begin_tx().await?;
        let _ = tx.update_node(raw_id, props).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Delete node (and its attached rels) using typed tx CRUD.
    pub async fn delete(&self, id: &M::Id) -> Result<()> {
        let raw_id: i64 = id.clone().into();

        let mut tx = self.backend.begin_tx().await?;
        tx.delete_node(raw_id).await?;
        tx.commit().await?;
        Ok(())
    }
}

pub struct NodeRepo<'a, T: GraphTx + Send, M> {
    tx: &'a mut Transaction<T>,
    _marker: std::marker::PhantomData<M>,
}

impl<'a, T: GraphTx + Send, M> NodeRepo<'a, T, M> {
    pub fn new(tx: &'a mut Transaction<T>) -> Self {
        Self { tx, _marker: std::marker::PhantomData }
    }
}
