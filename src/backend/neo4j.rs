use std::collections::BTreeMap;

use async_trait::async_trait;
use neo4rs::{BoltMap, BoltString, BoltType, Graph, Node, Query, Relation, Row, Txn, query};
use serde_json::Value;

use crate::backend::{
    BackendCapabilities, BackendIdType, BackendIdentity, GraphBackend, GraphTx, StoredNode,
    StoredRel,
};
use crate::dsl::{GraphQuery, KernelValue, NodeValue, QueryResult, RelValue, Return, VarId};
use crate::error::{GrmError, Result};
use crate::{QueryRow, graph_query_to_cypher};

#[derive(Debug, Clone)]
pub struct Neo4jConfig {
    pub uri: String,
    pub user: String,
    pub password: String,
}

#[derive(Clone)]
pub struct Neo4jBackend {
    graph: Graph,
}

pub struct Neo4jTx {
    tx: Txn,
}

impl Neo4jBackend {
    pub async fn connect(config: Neo4jConfig) -> Result<Self> {
        let graph = Graph::new(config.uri, config.user, config.password)
            .await
            .map_err(neo4j_err)?;
        Ok(Self { graph })
    }

    pub fn from_graph(graph: Graph) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl GraphBackend for Neo4jBackend {
    type Tx = Neo4jTx;

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            graph_query: true,
            string_query: true,
            transactions: true,
            read_your_writes: true,
            rollback: true,
        }
    }

    async fn execute_query(&self, query_text: &str, params: Value) -> Result<QueryResult> {
        let mut tx = self.begin_tx().await?;
        let rows = tx.execute_query(query_text, params).await?;
        tx.commit().await?;
        Ok(rows)
    }

    async fn begin_tx(&self) -> Result<Self::Tx> {
        let tx = self.graph.start_txn().await.map_err(neo4j_err)?;
        Ok(Neo4jTx { tx })
    }
}

impl BackendIdentity for Neo4jBackend {
    fn node_id_type(&self) -> BackendIdType {
        BackendIdType::Int64
    }
}

#[async_trait]
impl GraphTx for Neo4jTx {
    async fn execute_query(&mut self, query_text: &str, params: Value) -> Result<QueryResult> {
        let mut rows = self
            .tx
            .execute(apply_json_params(query(query_text), params)?)
            .await
            .map_err(neo4j_err)?;

        let mut result_rows = Vec::new();
        while let Some(row) = rows.next(self.tx.handle()).await.map_err(neo4j_err)? {
            result_rows.push(single_value_row(row)?);
        }
        Ok(QueryResult { rows: result_rows })
    }

    async fn execute_graph(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        let cypher = graph_query_to_cypher(q)?;
        let mut rows = self
            .tx
            .execute(apply_params(query(&cypher.text), &cypher.params)?)
            .await
            .map_err(neo4j_err)?;

        let mut result_rows = Vec::new();
        while let Some(row) = rows.next(self.tx.handle()).await.map_err(neo4j_err)? {
            result_rows.push(graph_query_row(q, row)?);
        }
        Ok(QueryResult { rows: result_rows })
    }

    async fn create_node(
        &mut self,
        labels: Vec<String>,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredNode> {
        let label_clause = labels
            .iter()
            .map(|label| format!(":{}", cypher_name(label)))
            .collect::<String>();
        let text = format!("CREATE (n{label_clause}) SET n += $props RETURN n");
        let mut rows = self
            .tx
            .execute(query(&text).param("props", props_to_bolt_map(&props)?))
            .await
            .map_err(neo4j_err)?;
        let row = self.one_row(&mut rows).await?;
        let node: Node = row.get("n").map_err(neo4j_value_err)?;
        Ok(stored_node(node)?)
    }

    async fn update_node(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        let mut rows = self
            .tx
            .execute(
                query("MATCH (n) WHERE id(n) = $id SET n += $props RETURN n")
                    .param("id", id)
                    .param("props", props_to_bolt_map(&props)?),
            )
            .await
            .map_err(neo4j_err)?;
        let row = self.optional_row(&mut rows).await?;
        row.map(|row| {
            let node: Node = row.get("n").map_err(neo4j_value_err)?;
            stored_node(node)
        })
        .transpose()
    }

    async fn delete_node(&mut self, id: i64) -> Result<()> {
        self.tx
            .run(query("MATCH (n) WHERE id(n) = $id DETACH DELETE n").param("id", id))
            .await
            .map_err(neo4j_err)
    }

    async fn find_node_by_id(&mut self, id: i64) -> Result<Option<StoredNode>> {
        let mut rows = self
            .tx
            .execute(query("MATCH (n) WHERE id(n) = $id RETURN n").param("id", id))
            .await
            .map_err(neo4j_err)?;
        let row = self.optional_row(&mut rows).await?;
        row.map(|row| {
            let node: Node = row.get("n").map_err(neo4j_value_err)?;
            stored_node(node)
        })
        .transpose()
    }

    async fn find_nodes_by_property(
        &mut self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<StoredNode>> {
        let text = format!("MATCH (n) WHERE n.{} = $value RETURN n", cypher_name(key));
        let mut rows = self
            .tx
            .execute(query(&text).param("value", json_to_bolt(value)?))
            .await
            .map_err(neo4j_err)?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next(self.tx.handle()).await.map_err(neo4j_err)? {
            let node: Node = row.get("n").map_err(neo4j_value_err)?;
            nodes.push(stored_node(node)?);
        }
        Ok(nodes)
    }

    async fn create_relationship(
        &mut self,
        from: i64,
        to: i64,
        rel_type: &str,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredRel> {
        let text = format!(
            "MATCH (a), (b) WHERE id(a) = $from AND id(b) = $to CREATE (a)-[r:{}]->(b) SET r += $props RETURN r",
            cypher_name(rel_type)
        );
        let mut rows = self
            .tx
            .execute(
                query(&text)
                    .param("from", from)
                    .param("to", to)
                    .param("props", props_to_bolt_map(&props)?),
            )
            .await
            .map_err(neo4j_err)?;
        let row = self.one_row(&mut rows).await?;
        let rel: Relation = row.get("r").map_err(neo4j_value_err)?;
        Ok(stored_rel(rel)?)
    }

    async fn update_relationship(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredRel>> {
        let mut rows = self
            .tx
            .execute(
                query("MATCH ()-[r]-() WHERE id(r) = $id SET r += $props RETURN r")
                    .param("id", id)
                    .param("props", props_to_bolt_map(&props)?),
            )
            .await
            .map_err(neo4j_err)?;
        let row = self.optional_row(&mut rows).await?;
        row.map(|row| {
            let rel: Relation = row.get("r").map_err(neo4j_value_err)?;
            stored_rel(rel)
        })
        .transpose()
    }

    async fn delete_relationship(&mut self, id: i64) -> Result<()> {
        self.tx
            .run(query("MATCH ()-[r]-() WHERE id(r) = $id DELETE r").param("id", id))
            .await
            .map_err(neo4j_err)
    }

    async fn outgoing(
        &mut self,
        from: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        self.neighbor_pairs(
            "MATCH (a)-[r{ty}]->(n) WHERE id(a) = $id RETURN r, n",
            from,
            rel_type,
        )
        .await
    }

    async fn incoming(
        &mut self,
        to: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        self.neighbor_pairs(
            "MATCH (n)-[r{ty}]->(a) WHERE id(a) = $id RETURN r, n",
            to,
            rel_type,
        )
        .await
    }

    async fn both(
        &mut self,
        node: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        self.neighbor_pairs(
            "MATCH (a)-[r{ty}]-(n) WHERE id(a) = $id RETURN r, n",
            node,
            rel_type,
        )
        .await
    }

    async fn commit(self) -> Result<()> {
        self.tx.commit().await.map_err(neo4j_err)
    }

    async fn rollback(self) -> Result<()> {
        self.tx.rollback().await.map_err(neo4j_err)
    }
}

impl Neo4jTx {
    async fn one_row(&mut self, rows: &mut neo4rs::RowStream) -> Result<Row> {
        rows.next(self.tx.handle())
            .await
            .map_err(neo4j_err)?
            .ok_or_else(|| GrmError::Constraint("Neo4j query did not return a row".into()))
    }

    async fn optional_row(&mut self, rows: &mut neo4rs::RowStream) -> Result<Option<Row>> {
        rows.next(self.tx.handle()).await.map_err(neo4j_err)
    }

    async fn neighbor_pairs(
        &mut self,
        template: &str,
        id: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let ty = rel_type
            .map(|rel_type| format!(":{}", cypher_name(rel_type)))
            .unwrap_or_default();
        let text = template.replace("{ty}", &ty);
        let mut rows = self
            .tx
            .execute(query(&text).param("id", id))
            .await
            .map_err(neo4j_err)?;

        let mut pairs = Vec::new();
        while let Some(row) = rows.next(self.tx.handle()).await.map_err(neo4j_err)? {
            let rel: Relation = row.get("r").map_err(neo4j_value_err)?;
            let node: Node = row.get("n").map_err(neo4j_value_err)?;
            pairs.push((stored_rel(rel)?, stored_node(node)?));
        }
        Ok(pairs)
    }
}

fn graph_query_row(q: &GraphQuery, row: Row) -> Result<QueryRow> {
    let rel_vars = q
        .matches
        .iter()
        .filter_map(|clause| match clause {
            crate::dsl::MatchClause::Hop(hop) => Some(hop.rel_var),
            crate::dsl::MatchClause::Node(_) => None,
        })
        .collect::<std::collections::BTreeSet<_>>();

    let mut values = BTreeMap::new();
    for var in q.bound_vars() {
        let name = var_name(var);
        let value = if rel_vars.contains(&var) {
            let rel: Relation = row.get(&name).map_err(neo4j_value_err)?;
            KernelValue::Rel(rel_value(rel)?)
        } else {
            let node: Node = row.get(&name).map_err(neo4j_value_err)?;
            KernelValue::Node(node_value(node)?)
        };
        values.insert(var, value);
    }

    match q.ret {
        Return::Node(var) => {
            if !matches!(values.get(&var), Some(KernelValue::Node(_))) {
                return Err(GrmError::Mapping(format!(
                    "Neo4j GraphQuery row return var {var:?} was not a node"
                )));
            }
        }
        Return::Rel(var) => {
            if !matches!(values.get(&var), Some(KernelValue::Rel(_))) {
                return Err(GrmError::Mapping(format!(
                    "Neo4j GraphQuery row return var {var:?} was not a relationship"
                )));
            }
        }
    }

    Ok(QueryRow { values })
}

fn single_value_row(row: Row) -> Result<QueryRow> {
    if let Ok(node) = row.get::<Node>("n") {
        return Ok(QueryRow {
            values: BTreeMap::from([(VarId(0), KernelValue::Node(node_value(node)?))]),
        });
    }
    if let Ok(rel) = row.get::<Relation>("r") {
        return Ok(QueryRow {
            values: BTreeMap::from([(VarId(0), KernelValue::Rel(rel_value(rel)?))]),
        });
    }
    Ok(QueryRow {
        values: BTreeMap::new(),
    })
}

fn stored_node(node: Node) -> Result<StoredNode> {
    Ok(StoredNode {
        id: node.id(),
        labels: node.labels().into_iter().map(str::to_string).collect(),
        props: node_props(&node)?,
    })
}

fn node_value(node: Node) -> Result<NodeValue> {
    Ok(NodeValue {
        id: node.id(),
        labels: node.labels().into_iter().map(str::to_string).collect(),
        props: node_props(&node)?,
    })
}

fn stored_rel(rel: Relation) -> Result<StoredRel> {
    Ok(StoredRel {
        id: rel.id(),
        rel_type: rel.typ().to_string(),
        from: rel.start_node_id(),
        to: rel.end_node_id(),
        props: rel_props(&rel)?,
    })
}

fn rel_value(rel: Relation) -> Result<RelValue> {
    Ok(RelValue {
        id: rel.id(),
        ty: rel.typ().to_string(),
        from: rel.start_node_id(),
        to: rel.end_node_id(),
        props: rel_props(&rel)?,
    })
}

fn node_props(node: &Node) -> Result<BTreeMap<String, Value>> {
    let mut props = BTreeMap::new();
    for key in node.keys() {
        props.insert(key.to_string(), node.get(key).map_err(neo4j_value_err)?);
    }
    Ok(props)
}

fn rel_props(rel: &Relation) -> Result<BTreeMap<String, Value>> {
    let mut props = BTreeMap::new();
    for key in rel.keys() {
        props.insert(key.to_string(), rel.get(key).map_err(neo4j_value_err)?);
    }
    Ok(props)
}

fn apply_json_params(mut query: Query, params: Value) -> Result<Query> {
    let Value::Object(params) = params else {
        return Ok(query);
    };
    for (key, value) in params {
        query = query.param(&key, json_to_bolt(&value)?);
    }
    Ok(query)
}

fn apply_params(mut query: Query, params: &BTreeMap<String, Value>) -> Result<Query> {
    for (key, value) in params {
        query = query.param(key, json_to_bolt(value)?);
    }
    Ok(query)
}

fn props_to_bolt_map(props: &BTreeMap<String, Value>) -> Result<BoltType> {
    let mut map = BoltMap::new();
    for (key, value) in props {
        map.put(BoltString::from(key.clone()), json_to_bolt(value)?);
    }
    Ok(BoltType::Map(map))
}

fn json_to_bolt(value: &Value) -> Result<BoltType> {
    match value {
        Value::Bool(value) => Ok(BoltType::from(*value)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(BoltType::from(value))
            } else if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
                Ok(BoltType::from(value))
            } else if let Some(value) = value.as_f64() {
                Ok(BoltType::from(value))
            } else {
                Err(GrmError::Mapping(format!(
                    "unsupported Neo4j numeric value: {value}"
                )))
            }
        }
        Value::String(value) => Ok(BoltType::from(value.clone())),
        Value::Object(props) => {
            let mut map = BoltMap::new();
            for (key, value) in props {
                map.put(BoltString::from(key.clone()), json_to_bolt(value)?);
            }
            Ok(BoltType::Map(map))
        }
        Value::Null | Value::Array(_) => Err(GrmError::Mapping(format!(
            "unsupported Neo4j parameter value: {value}"
        ))),
    }
}

fn cypher_name(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn var_name(var: VarId) -> String {
    format!("v{}", var.0)
}

fn neo4j_err(err: neo4rs::Error) -> GrmError {
    GrmError::Backend(format!("Neo4j error: {err}"))
}

fn neo4j_value_err(err: neo4rs::DeError) -> GrmError {
    GrmError::Mapping(format!("Neo4j value mapping error: {err}"))
}
