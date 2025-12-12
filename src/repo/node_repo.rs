use crate::{
    GraphBackend, NodeModel, Query, QueryResult, error::{GrmError, Result}
};

use serde_json::json;
use std::collections::BTreeMap;

fn decode_nodes<M: NodeModel>(qr: QueryResult) -> Result<Vec<M>> {
    let mut out = Vec::with_capacity(qr.rows.len());

    for row in qr.rows {
        let v = row.values.get("n")
            .ok_or_else(|| GrmError::Backend("execute_graph row missing key 'n'".into()))?;

        let id = v.get("id")
            .and_then(|x| x.as_i64())
            .ok_or_else(|| GrmError::Backend("node missing/invalid id".into()))?;

        let props_obj = v.get("props")
            .and_then(|x| x.as_object())
            .ok_or_else(|| GrmError::Backend("node missing/invalid props".into()))?;

        // Convert serde_json::Value -> your crate::dsl::Value (if different)
        // If your NodeModel expects BTreeMap<String, crate::dsl::Value>, map it here.
        let props = props_obj.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<BTreeMap<String, serde_json::Value>>();

        // If NodeModel::from_properties expects your Value type, convert accordingly.
        // For now, assuming NodeModel uses serde_json::Value like StoredNode does:
        out.push(M::from_properties(id.into(), props)?);
    }

    Ok(out)
}

async fn execute_node_query<B, M>(repo: &NodeRepository<B, M>, q: Query<M>) -> Result<Vec<M>>
where
    B: GraphBackend,
    M: NodeModel,
{
    let gq = q.compile_to_graph();

    // Execute via typed IR
    let result = repo.backend.execute_graph(&gq).await?;

    // Decode rows -> Vec<M>
    // This depends on what QueryResult looks like for execute_graph.
    // The simplest: return rows with a single key "n" containing {id, props, labels}.
    decode_nodes::<M>(result)
}

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

    pub async fn query(&self, q: Query<M>) -> Result<Vec<M>> {
        execute_node_query(self, q).await
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
