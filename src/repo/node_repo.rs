use crate::{
    CompareOp, GraphBackend, NodeModel, PropertyFilter, Query, QueryKind,
    error::{GrmError, Result},
};

use serde_json::{Value, json};
use std::collections::BTreeMap;

fn apply_paging<N>(mut items: Vec<N>, offset: Option<usize>, limit: Option<usize>) -> Vec<N> {
    let start = offset.unwrap_or(0);
    if start >= items.len() {
        return Vec::new();
    }

    let end = if let Some(limit) = limit {
        start.saturating_add(limit).min(items.len())
    } else {
        items.len()
    };

    items.drain(..start); // drop items before offset
    items.truncate(end - start);
    items
}

fn numeric_cmp<F>(a: &Value, b: &Value, cmp: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (a.as_f64(), b.as_f64()) {
        (Some(la), Some(rb)) => cmp(la, rb),
        _ => false,
    }
}

pub fn node_matches_filters<N: NodeModel>(node: &N, filters: &[PropertyFilter]) -> bool {
    if filters.is_empty() {
        return true;
    }

    let props = node.to_properties();

    for f in filters {
        let value = match props.get(f.key) {
            Some(v) => v,
            None => return false,
        };

        let ok = match f.op {
            CompareOp::Eq => value == &f.value,
            CompareOp::Ne => value != &f.value,

            // Very naive numeric comparisons, works if both are numbers
            CompareOp::Gt => numeric_cmp(value, &f.value, |a, b| a > b),
            CompareOp::Ge => numeric_cmp(value, &f.value, |a, b| a >= b),
            CompareOp::Lt => numeric_cmp(value, &f.value, |a, b| a < b),
            CompareOp::Le => numeric_cmp(value, &f.value, |a, b| a <= b),

            // String CONTAINS
            CompareOp::Contains => {
                if let (Some(lhs), Some(rhs)) = (value.as_str(), f.value.as_str()) {
                    lhs.contains(rhs)
                } else {
                    false
                }
            }
        };

        if !ok {
            // this filter failed → node doesn't match
            return false; // reject node on first failing filter
        }
    }

    true
}

async fn execute_node_query<B, M>(repo: &NodeRepository<B, M>, q: Query<M>) -> Result<Vec<M>>
where
    B: GraphBackend,
    M: NodeModel,
{
    match q.kind {
        QueryKind::MatchNode {
            pattern,
            limit,
            offset,
        } => {
            // 1) Fast path: ID-based match
            if let Some(id) = pattern.id.clone() {
                if let Some(node) = repo.find_by_id(&id).await? {
                    let mut v = vec![node];
                    v.retain(|n| node_matches_filters(n, &pattern.property_filters));
                    return Ok(apply_paging(v, offset, limit));
                } else {
                    return Ok(Vec::new());
                }
            }

            // 2) No ID → use the first Eq filter as backend filter (if any)
            let mut eq_filters: Vec<PropertyFilter> = Vec::new();
            let mut non_eq_filters: Vec<PropertyFilter> = Vec::new();

            for f in pattern.property_filters {
                match f.op {
                    CompareOp::Eq => eq_filters.push(f),
                    _ => non_eq_filters.push(f),
                }
            }

            let primary_eq = eq_filters.first();

            // If we have at least one Eq filter, use it as the backend filter.
            let mut results: Vec<M> = if let Some(f) = primary_eq {
                repo.find_by(f.key, &f.value).await?
            } else {
                // No id, no Eq filter → currently unsupported without "find all".
                // For now, just return empty.
                Vec::new()
            };

            // 3) Apply *all* filters in-memory (so additional Eq filters still matter).
            if !eq_filters.is_empty() || !non_eq_filters.is_empty() {
                let mut all_filters = eq_filters;
                all_filters.extend(non_eq_filters);
                results.retain(|n| node_matches_filters(n, &all_filters));
            }

            // 4) Apply offset/limit in-memory
            Ok(apply_paging(results, offset, limit))
        }
    }
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
