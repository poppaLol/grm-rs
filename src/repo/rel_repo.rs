use std::collections::BTreeMap;
use std::marker::PhantomData;

use serde_json::Value as JsonValue;

use crate::{
    autocommit,
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

    /// Get all outgoing relationships of this type from a given node,
    /// returning (relationship, target_node) pairs. (typed)
    pub async fn outgoing_from(
        &self,
        from_id: &<R::From as NodeModel>::Id,
    ) -> Result<Vec<(R, R::To)>> {
        autocommit!(self.backend, |tx| {
            let mut repo_tx = RelRepositoryTx::<B::Tx, R>::new(&mut tx);
            repo_tx.outgoing_from(from_id).await
        })
    }

    pub async fn incoming_to(
        &self,
        to_id: &<R::To as NodeModel>::Id,
    ) -> Result<Vec<(R, R::From)>> {
        autocommit!(self.backend, |tx| {
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
            .create_relationship(from_raw, to_raw, R::TYPE.to_string(), props)
            .await?;

        rel.set_id(stored.id.into());
        Ok(())
    }

    pub async fn outgoing_from(
        &mut self,
        from_id: &<R::From as NodeModel>::Id,
    ) -> Result<Vec<(R, R::To)>> {
        let from_raw: i64 = from_id.clone().into();

        let pairs = self.tx.tx_mut()?.outgoing(from_raw, Some(R::TYPE)).await?;

        let mut out = Vec::with_capacity(pairs.len());

        for (stored_rel, stored_node) in pairs {
            // Decode relationship model
            let rel_id: R::Id = stored_rel.id.into();
            let rel_model = R::from_properties(rel_id, stored_rel.props)?;

            // Optional: enforce target node labels match R::To
            if !labels_match::<R::To>(&stored_node) {
                continue;
            }

            // Decode target node model
            let node_id: <R::To as NodeModel>::Id = stored_node.id.into();
            let node_model = <R::To as NodeModel>::from_properties(node_id, stored_node.props)?;

            out.push((rel_model, node_model));
        }

        Ok(out)
    }

        pub async fn incoming_to(
        &mut self,
        to_id: &<R::To as NodeModel>::Id,
    ) -> Result<Vec<(R, R::From)>> {
        let to_raw: i64 = to_id.clone().into();

        let pairs = self.tx.tx_mut()?.incoming(to_raw, Some(R::TYPE)).await?;

        let mut out = Vec::with_capacity(pairs.len());

        for (stored_rel, stored_node) in pairs {
            // Decode relationship model
            let rel_id: R::Id = stored_rel.id.into();
            let rel_model = R::from_properties(rel_id, stored_rel.props)?;

            // Enforce node labels match R::From (because we're decoding R::From from stored_node)
            if !labels_match::<R::From>(&stored_node) {
                continue;
            }

            // Decode source node model
            let node_id: <R::From as NodeModel>::Id = stored_node.id.into();
            let node_model =
                <R::From as NodeModel>::from_properties(node_id, stored_node.props)?;

            out.push((rel_model, node_model));
        }

        Ok(out)
    }
}
