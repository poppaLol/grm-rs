use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::dsl::KernelValue;
use serde_json::Value as JsonValue;

use crate::GraphQuery;
use crate::backend::{GraphBackend, GraphTx};
use crate::dsl::{Query, QueryResult};
use crate::error::{GrmError, Result};
use crate::model::NodeModel;

/// Decode QueryResult rows (from execute_graph) into models.
#[allow(unreachable_patterns)]
fn decode_nodes<M: NodeModel>(gq: &GraphQuery, qr: QueryResult) -> Result<Vec<M>> {
    let mut out = Vec::with_capacity(qr.rows.len());

    for row in qr.rows {
        let v = row
            .get_returned(gq)
            .ok_or_else(|| GrmError::Backend("execute_graph row missing return var".into()))?;

        let node = match v {
            KernelValue::Node(n) => n,
            //this is unreachable at this time - but retained for future - TODO
            _ => return Err(GrmError::Backend("expected node return value".into())),
        };

        out.push(M::from_properties(node.id.into(), node.props.clone())?);
    }

    Ok(out)
}

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

    pub async fn query_from<R>(&self, q: Query<R>) -> Result<Vec<M>>
    where
        R: NodeModel,
    {
        let gq = q.compile_to_graph();
        let qr = self.backend.execute_graph(&gq).await?;
        decode_nodes::<M>(&gq, qr)
    }

    /// Option A sugar: Query<M> -> compile_to_graph -> execute_graph -> decode.
    pub async fn query(&self, q: Query<M>) -> Result<Vec<M>> {
        let gq = q.compile_to_graph();
        let qr = self.backend.execute_graph(&gq).await?;
        decode_nodes::<M>(&gq, qr)
    }

    // needed to prove out some functionality - TODO - replace and bring this together with other funcs
    pub async fn query_kernel_from<R>(&self, q: Query<R>) -> Result<(GraphQuery, QueryResult)>
    where
        R: NodeModel,
    {
        let gq = q.compile_to_graph();
        let qr = self.backend.execute_graph(&gq).await?;
        Ok((gq, qr))
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
