use std::collections::BTreeMap;
use std::marker::PhantomData;

use serde_json::Value as JsonValue;

use crate::{
    backend::{GraphBackend, GraphTx}, // adjust path if GraphTx is elsewhere
    error::Result,
    model::{NodeModel, RelModel}, repo::labels::labels_match,
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
        let from_raw: i64 = from_id.clone().into();
        let to_raw: i64 = to_id.clone().into();
        let props: BTreeMap<String, JsonValue> = rel.to_properties();

        let mut tx = self.backend.begin_tx().await?;
        let stored = tx
            .create_relationship(from_raw, to_raw, R::TYPE.to_string(), props)
            .await?;
        tx.commit().await?;

        rel.set_id(stored.id.into());
        Ok(())
    }

    /// Get all outgoing relationships of this type from a given node,
    /// returning (relationship, target_node) pairs. (typed)
    pub async fn outgoing_from(
        &self,
        from_id: &<R::From as NodeModel>::Id,
    ) -> Result<Vec<(R, R::To)>> {
        let from_raw: i64 = from_id.clone().into();

        let mut tx = self.backend.begin_tx().await?;
        let pairs = tx.outgoing(from_raw, Some(R::TYPE)).await?;

        let mut out = Vec::with_capacity(pairs.len());

        for (stored_rel, stored_node) in pairs {
            // Decode relationship model
            let rel_id: R::Id = stored_rel.id.into();
            let rel_model = R::from_properties(rel_id, stored_rel.props)?;

            // Optional: enforce target node labels match R::To
            if !labels_match(&stored_node.labels, <R::To as NodeModel>::LABELS) {
                continue;
            }

            // Decode target node model
            let node_id: <R::To as NodeModel>::Id = stored_node.id.into();
            let node_model = <R::To as NodeModel>::from_properties(node_id, stored_node.props)?;

            out.push((rel_model, node_model));
        }

        tx.commit().await?;
        Ok(out)
    }

    pub async fn incoming_to(&self, to_id: &<R::To as NodeModel>::Id) -> Result<Vec<(R, R::From)>> {
        let to_raw: i64 = to_id.clone().into();

        let mut tx = self.backend.begin_tx().await?;
        let pairs = tx.incoming(to_raw, Some(R::TYPE)).await?;

        let mut out = Vec::with_capacity(pairs.len());

        for (stored_rel, stored_node) in pairs {
            // Decode relationship model
            let rel_id: R::Id = stored_rel.id.into();
            let rel_model = R::from_properties(rel_id, stored_rel.props)?;

            // Enforce node labels match R::From
            // Optional: enforce target node labels match R::To
            if !labels_match(&stored_node.labels, <R::From as NodeModel>::LABELS) {
                continue;
            }

            // Decode source node model
            let node_id: <R::From as NodeModel>::Id = stored_node.id.into();
            let node_model = <R::From as NodeModel>::from_properties(node_id, stored_node.props)?;

            out.push((rel_model, node_model));
        }

        tx.commit().await?;
        Ok(out)
    }
}
