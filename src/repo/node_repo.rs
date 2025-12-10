use serde_json::json;
use std::collections::BTreeMap;

use crate::{
    GraphBackend, NodeModel,
    error::{GrmError, Result},
};

pub struct NodeRepository<B, M>
where
    B: GraphBackend,
    M: NodeModel,
{
    backend: B,
    _marker: std::marker::PhantomData<M>,
}

impl<B, M> NodeRepository<B, M>
where
    B: GraphBackend + Clone,
    M: NodeModel,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            _marker: std::marker::PhantomData,
        }
    }

    /// INSERT node using pseudo-Cypher
    pub async fn create(&self, model: &mut M) -> Result<()> {
        // 1. Prepare labels & props to send to the backend
        let labels = M::LABELS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let props: BTreeMap<String, serde_json::Value> = model.to_properties();

        // 2. Execute our pseudo-Cypher "CREATE" query
        let result = self
            .backend
            .execute_query(
                "CREATE (n) RETURN n",
                json!({
                    "labels": labels,
                    "props": props,
                }),
            )
            .await?;

        // 3. Extract the generated ID from the result and set it on the model
        let row = result
            .rows
            .get(0)
            .ok_or_else(|| GrmError::Backend("CREATE did not return a row".into()))?;

        let n_json = row
            .values
            .get("n")
            .ok_or_else(|| GrmError::Backend("CREATE row missing 'n' key".into()))?;

        let raw_id = n_json["id"]
            .as_i64()
            .ok_or_else(|| GrmError::Backend("node id is not an i64".into()))?;

        let id: M::Id = raw_id.into(); // thanks to `From<i64>` bound on M::Id
        model.set_id(id);

        Ok(())
    }

    /// MATCH node by ID
    pub async fn find_by_id(&self, id: &M::Id) -> Result<Option<M>> {
        let raw_id: i64 = (*id).clone().into(); // thanks to `Into<i64>` bound on M::Id

        let result = self
            .backend
            .execute_query(
                "MATCH (n) WHERE id(n) = $id RETURN n",
                json!({
                    "id": raw_id,
                }),
            )
            .await?;

        if let Some(row) = result.rows.get(0) {
            let n_json = row
                .values
                .get("n")
                .ok_or_else(|| GrmError::Backend("MATCH row missing 'n' key".into()))?;

            let raw_id = n_json["id"]
                .as_i64()
                .ok_or_else(|| GrmError::Backend("node id is not an i64".into()))?;

            let id: M::Id = raw_id.into();

            let props_map = n_json["props"]
                .as_object()
                .ok_or_else(|| GrmError::Backend("node props is not an object".into()))?
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>();

            let model = M::from_properties(id, props_map)?;
            Ok(Some(model))
        } else {
            Ok(None)
        }
    }

    pub async fn find_by(&self, key: &str, value: &serde_json::Value) -> Result<Vec<M>> {
        let result = self
            .backend
            .execute_query(
                "MATCH (n) WHERE n.$key = $value RETURN n",
                json!({
                    "key": key,
                    "value": value,
                }),
            )
            .await?;

        let mut out = vec![];

        for row in result.rows {
            let json = row.values.get("n").unwrap();

            let raw_id = json["id"].as_i64().unwrap();
            let id: M::Id = raw_id.into();

            let props = json["props"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let model = M::from_properties(id, props)?;
            out.push(model);
        }

        Ok(out)
    }

    pub async fn update(&self, model: &M) -> Result<()> {
        use serde_json::json;
        use std::collections::BTreeMap;

        let raw_id: i64 = model.id().clone().into();
        let props: BTreeMap<String, serde_json::Value> = model.to_properties();

        let _result = self
            .backend
            .execute_query(
                "MATCH (n) WHERE id(n) = $id SET n += $props RETURN n",
                json!({
                    "id": raw_id,
                    "props": props,
                }),
            )
            .await?;

        Ok(())
    }

    pub async fn delete(&self, id: &M::Id) -> Result<()> {
        use serde_json::json;

        let raw_id: i64 = id.clone().into();

        let _ = self
            .backend
            .execute_query(
                "MATCH (n) WHERE id(n) = $id DELETE n",
                json!({
                    "id": raw_id,
                }),
            )
            .await?;

        Ok(())
    }
}
