use std::collections::BTreeMap;
use std::marker::PhantomData;

use serde_json::Value as JsonValue;

use crate::{
    autocommit, autoread,
    backend::{GraphBackend, GraphTx},
    client::Transaction,
    error::Result,
    labels_match,
    model::{NodeModel, RelModel},
};

pub struct RelRepository<B, R>
where
    B: GraphBackend,
    R: RelModel,
{
    backend: B,
    _marker: PhantomData<R>,
}

impl<B, R> RelRepository<B, R>
where
    B: GraphBackend + Clone,
    R: RelModel,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            _marker: PhantomData,
        }
    }

    /// Create a relationship between two nodes (typed).
    pub async fn create_between(
        &self,
        from_id: &<R::From as NodeModel>::Id,
        to_id: &<R::To as NodeModel>::Id,
        rel: &mut R,
    ) -> Result<()> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = RelRepositoryTx::<B::Tx, R>::new(&mut tx);
            repo_tx.create_between(from_id, to_id, rel).await
        })
    }

    /// Create a relationship between two nodes by i64 IDs.
    pub async fn create_between_i64(&self, from_id: i64, to_id: i64, rel: &mut R) -> Result<()> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = RelRepositoryTx::<B::Tx, R>::new(&mut tx);
            repo_tx.create_between_i64(from_id, to_id, rel).await
        })
    }

    /// Get all outgoing relationships of this type from a given node,
    /// returning (relationship, target_node_id) pairs. (typed)
    pub async fn outgoing_from(
        &self,
        from_id: &<R::From as NodeModel>::Id,
    ) -> Result<Vec<(R, Option<i64>)>> {
        autoread!(self.backend, |tx| {
            let mut repo_tx = RelRepositoryTx::<B::Tx, R>::new(&mut tx);
            repo_tx.outgoing_from(from_id).await
        })
    }

    pub async fn incoming_to(
        &self,
        to_id: &<R::To as NodeModel>::Id,
    ) -> Result<Vec<(R, Option<i64>)>> {
        autoread!(self.backend, |tx| {
            let mut repo_tx = RelRepositoryTx::<B::Tx, R>::new(&mut tx);
            repo_tx.incoming_to(to_id).await
        })
    }
}

pub struct RelRepositoryTx<'a, T, R>
where
    T: GraphTx + Send,
    R: RelModel,
{
    tx: &'a mut Transaction<T>,
    _marker: std::marker::PhantomData<R>,
}

impl<'a, T, R> RelRepositoryTx<'a, T, R>
where
    T: GraphTx + Send,
    R: RelModel,
{
    pub fn new(tx: &'a mut Transaction<T>) -> Self {
        Self {
            tx,
            _marker: std::marker::PhantomData,
        }
    }

    pub async fn create_between(
        &mut self,
        from_id: &<R::From as NodeModel>::Id,
        to_id: &<R::To as NodeModel>::Id,
        rel: &mut R,
    ) -> Result<()> {
        let from_raw: i64 = from_id.clone().into();
        let to_raw: i64 = to_id.clone().into();
        let props: BTreeMap<String, JsonValue> = rel.to_properties();

        let stored = self
            .tx
            .tx_mut()?
            .create_relationship(from_raw, to_raw, R::TYPE, props)
            .await?;

        rel.set_id(stored.id.into());
        rel.set_from(from_raw);
        rel.set_to(to_raw);
        Ok(())
    }

    pub async fn create_between_i64(
        &mut self,
        from_id: i64,
        to_id: i64,
        rel: &mut R,
    ) -> Result<()> {
        let props: BTreeMap<String, JsonValue> = rel.to_properties();

        let stored = self
            .tx
            .tx_mut()?
            .create_relationship(from_id, to_id, R::TYPE, props)
            .await?;

        rel.set_id(stored.id.into());
        rel.set_from(from_id);
        rel.set_to(to_id);
        Ok(())
    }

    pub async fn outgoing_from(
        &mut self,
        from_id: &<R::From as NodeModel>::Id,
    ) -> Result<Vec<(R, Option<i64>)>> {
        let from_raw: i64 = from_id.clone().into();

        let pairs = self.tx.tx_mut()?.outgoing(from_raw, Some(R::TYPE)).await?;

        let mut out = Vec::with_capacity(pairs.len());

        for (stored_rel, stored_node) in pairs {
            // Decode relationship model
            let rel_id: R::Id = stored_rel.id.into();
            let rel_model = R::from_parts(
                rel_id,
                stored_rel.from.into(),
                stored_rel.to.into(),
                stored_rel.props,
            )?;

            // Optional: enforce target node labels match R::To
            if !labels_match::<R::To>(&stored_node) {
                continue;
            }

            // Return only the endpoint ID, not the full node model
            out.push((rel_model, Some(stored_node.id.into())));
        }

        Ok(out)
    }

    pub async fn incoming_to(
        &mut self,
        to_id: &<R::To as NodeModel>::Id,
    ) -> Result<Vec<(R, Option<i64>)>> {
        let to_raw: i64 = to_id.clone().into();

        let pairs = self.tx.tx_mut()?.incoming(to_raw, Some(R::TYPE)).await?;

        let mut out = Vec::with_capacity(pairs.len());

        for (stored_rel, stored_node) in pairs {
            // Decode relationship model
            let rel_id: R::Id = stored_rel.id.into();
            let rel_model = R::from_parts(rel_id, stored_rel.from.into(), stored_rel.to.into(), stored_rel.props)?;

            // Enforce node labels match R::From (because we're decoding R::From from stored_node)
            if !labels_match::<R::From>(&stored_node) {
                continue;
            }

            // Return only the endpoint ID, not the full node model
            out.push((rel_model, Some(stored_node.id.into())));
        }

        Ok(out)
    }
}
