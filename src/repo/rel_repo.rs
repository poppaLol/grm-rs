use serde_json::json;
use std::collections::BTreeMap;

use crate::{
    GraphBackend, NodeModel, RelModel,
    error::{GrmError, Result},
};

pub struct RelRepository<B, R>
where
    B: GraphBackend,
    R: RelModel,
{
    backend: B,
    _marker: std::marker::PhantomData<R>,
}

impl<B, R> RelRepository<B, R>
where
    B: GraphBackend + Clone,
    R: RelModel,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a relationship between two nodes
    pub async fn create_between(
        &self,
        from_id: &<R::From as NodeModel>::Id,
        to_id: &<R::To as NodeModel>::Id,
        rel: &mut R,
    ) -> Result<()> {
        let from_raw: i64 = from_id.clone().into();
        let to_raw: i64 = to_id.clone().into();
        let props: BTreeMap<String, serde_json::Value> = rel.to_properties();

        let result = self
            .backend
            .execute_query(
                "MATCH (a), (b) \
             WHERE id(a) = $from AND id(b) = $to \
             CREATE (a)-[r]->(b) \
             RETURN r",
                serde_json::json!({
                    "from": from_raw,
                    "to": to_raw,
                    "type": R::TYPE,
                    "props": props,
                }),
            )
            .await?;

        let row = result
            .rows
            .get(0)
            .ok_or_else(|| GrmError::Backend("CREATE rel did not return row".into()))?;

        let r_json = row
            .values
            .get("r")
            .ok_or_else(|| GrmError::Backend("CREATE rel row missing 'r'".into()))?;

        let raw_id = r_json["id"]
            .as_i64()
            .ok_or_else(|| GrmError::Backend("rel id is not i64".into()))?;

        let id: R::Id = raw_id.into();
        rel.set_id(id);

        Ok(())
    }

    /// Get all outgoing relationships of this type from a given node,
    /// returning (relationship, target_node) pairs.
    pub async fn outgoing_from(
        &self,
        from_id: &<R::From as NodeModel>::Id,
    ) -> Result<Vec<(R, R::To)>> {
        let from_raw: i64 = from_id.clone().into();

        let result = self
            .backend
            .execute_query(
                "MATCH (A)-[R]->(B) WHERE id(a) = $from RETURN r, b",
                json!({
                    "from": from_raw,
                    "type": R::TYPE,
                }),
            )
            .await?;

        let mut out = Vec::new();

        for row in result.rows {
            let r_json = row
                .values
                .get("r")
                .ok_or_else(|| GrmError::Backend("row missing 'r'".into()))?;

            let rel_id_raw = r_json["id"]
                .as_i64()
                .ok_or_else(|| GrmError::Backend("rel id not i64".into()))?;
            let rel_id: R::Id = rel_id_raw.into();

            let rel_props = r_json["props"]
                .as_object()
                .ok_or_else(|| GrmError::Backend("rel props not object".into()))?
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>();

            let rel_model = R::from_properties(rel_id, rel_props)?;

            let b_json = row
                .values
                .get("b")
                .ok_or_else(|| GrmError::Backend("row missing 'b'".into()))?;

            let node_id_raw = b_json["id"]
                .as_i64()
                .ok_or_else(|| GrmError::Backend("node id not i64".into()))?;
            let node_id: <R::To as NodeModel>::Id = node_id_raw.into();

            let node_props = b_json["props"]
                .as_object()
                .ok_or_else(|| GrmError::Backend("node props not object".into()))?
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>();

            let node_model = R::To::from_properties(node_id, node_props)?;

            out.push((rel_model, node_model));
        }

        Ok(out)
    }
}
