use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::{DecodeFromRow, autocommit, labels_match};
use crate::backend::{GraphBackend, GraphTx};
use crate::client::{QueryExecution, Transaction};
use crate::dsl::Query;
use crate::error::Result;
use crate::model::NodeModel;

/// Decode a StoredNode into M.
fn decode_stored_node<M: NodeModel>(id: i64, props: BTreeMap<String, JsonValue>) -> Result<M> {
    M::from_properties(id.into(), props)
}

pub async fn create_helper<T, M>(tx: &mut T, model: &mut M) -> Result<()>
where
    T: GraphTx + Send,
    M: NodeModel,
{
    let labels = M::LABELS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let props: BTreeMap<String, JsonValue> = model.to_properties();
    let stored = tx.create_node(labels, props).await?;
    model.set_id(stored.id.into());
    Ok(())
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
        autocommit!(self.backend, |tx| {
            let mut repo_tx = NodeRepositoryTx::<B::Tx, M>::new(&mut tx);
            repo_tx.execute(q).await
        })
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
        create_helper(&mut tx, model).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Find a node by internal id using typed tx CRUD.
    pub async fn find_by_id(&self, id: &M::Id) -> Result<Option<M>> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = NodeRepositoryTx::<B::Tx, M>::new(&mut tx);
            repo_tx.find_by_id(id).await
        })
    }

    /// Find nodes by a single property equality using typed tx CRUD.
    /// Note: returns all matches; not unique.
    pub async fn find_by(&self, key: &str, value: &JsonValue) -> Result<Vec<M>> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = NodeRepositoryTx::<B::Tx, M>::new(&mut tx);
            repo_tx.find_by(key, value).await
        })
    }

    /// Update node properties (SET += semantics) using typed tx CRUD.
    pub async fn update(&self, model: &M) -> Result<()> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = NodeRepositoryTx::<B::Tx, M>::new(&mut tx);
            repo_tx.update(model).await
        })
    }

    /// Delete node (and its attached rels) using typed tx CRUD.
    pub async fn delete(&self, id: &M::Id) -> Result<()> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = NodeRepositoryTx::<B::Tx, M>::new(&mut tx);
            repo_tx.delete(id).await
        })
    }
}

pub struct NodeRepositoryTx<'a, T: GraphTx + Send, M> {
    tx: &'a mut Transaction<T>,
    _marker: std::marker::PhantomData<M>,
}

impl<'a, T, M> NodeRepositoryTx<'a, T, M>
where
    T: GraphTx + Send,
    M: NodeModel,
{
    pub fn new(tx: &'a mut Transaction<T>) -> Self {
        Self {
            tx,
            _marker: std::marker::PhantomData,
        }
    }

    pub async fn create(&mut self, model: &mut M) -> Result<()> {
        let tx = self.tx.tx_mut()?;
        create_helper(tx, model).await
    }

    pub async fn execute<R: NodeModel>(&mut self, q: Query<R>) -> Result<QueryExecution> {
        self.tx.execute(q).await
    }

    pub async fn fetch<R: NodeModel, D: DecodeFromRow>(&mut self, q: Query<R>) -> Result<Vec<D>> {
        let exec = self.tx.execute(q).await?;
        exec.decode_all::<D>()
    }

    pub async fn find_by_id(&mut self, id: &M::Id) -> Result<Option<M>> {
        let raw_id: i64 = id.clone().into();

        let stored_opt = self.tx.tx_mut()?.find_node_by_id(raw_id).await?;

        match stored_opt {
            Some(stored) => {
                // Defensive label check (optional)
                let ok = labels_match::<M>(&stored);
                if !ok {
                    return Ok(None);
                }

                Ok(Some(decode_stored_node::<M>(stored.id, stored.props)?))
            }
            None => Ok(None),
        }
    }

    pub async fn find_by(&mut self, key: &str, value: &JsonValue) -> Result<Vec<M>> {
        let stored = self
            .tx
            .tx_mut()?
            .find_nodes_by_property(key, value)
            .await?;

        let mut out = Vec::with_capacity(stored.len());
        for n in stored {
            // Optional defensive label check
            let ok = labels_match::<M>(&n);
            if !ok {
                continue;
            }
            out.push(decode_stored_node::<M>(n.id, n.props)?);
        }
        Ok(out)
    }

    pub async fn update(&mut self, model: &M) -> Result<()> {
        let raw_id: i64 = model.id().clone().into();
        let props: BTreeMap<String, JsonValue> = model.to_properties();

        self.tx.tx_mut()?.update_node(raw_id, props).await?;
        Ok(())
    }

    pub async fn delete(&mut self, id: &M::Id) -> Result<()> {
        let raw_id: i64 = id.clone().into();

        self.tx.tx_mut()?.delete_node(raw_id).await?;
        Ok(())
    }
}
