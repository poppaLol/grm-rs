use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use crate::backend::{GraphBackend, GraphTx, QueryResult, QueryRow};
use crate::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use crate::dsl::{apply_paging, numeric_cmp};
use crate::error::{GrmError, Result};
use crate::{CompareOp, PropertyFilter};

#[derive(Debug, Clone)]
pub struct StoredNode {
    pub id: i64,
    pub labels: Vec<String>,
    pub props: BTreeMap<String, Value>,
}
#[derive(Debug, Clone)]
pub struct StoredRel {
    pub id: i64,
    pub rel_type: String,
    pub from: i64,
    pub to: i64,
    pub props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
struct GraphStore {
    next_node_id: i64,
    next_rel_id: i64,
    nodes: BTreeMap<i64, StoredNode>,
    rels: BTreeMap<i64, StoredRel>,
}

impl Default for GraphStore {
    fn default() -> Self {
        Self {
            next_node_id: 1,
            next_rel_id: 1,
            nodes: BTreeMap::new(),
            rels: BTreeMap::new(),
        }
    }
}

impl GraphStore {
    fn clone_store(&self) -> Self {
        Self {
            next_node_id: self.next_node_id,
            next_rel_id: self.next_rel_id,
            nodes: self.nodes.clone(),
            rels: self.rels.clone(),
        }
    }
}

#[derive(Clone)]
pub struct InMemoryBackend {
    store: Arc<Mutex<GraphStore>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(GraphStore::default())),
        }
    }
}

pub struct InMemoryTx {
    store: Arc<Mutex<GraphStore>>,
    working_copy: GraphStore,
    committed: bool,
}

impl InMemoryTx {
    fn new(store: Arc<Mutex<GraphStore>>) -> Self {
        let snapshot = store.lock().unwrap().clone_store();
        Self {
            store,
            working_copy: snapshot,
            committed: false,
        }
    }

    pub fn execute_graph_query(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        // ---- Helpers (kept local to avoid plumbing) ----

        fn labels_match(node_labels: &[String], required: &'static [&'static str]) -> bool {
            // Require all labels in `required` to be present on node.
            required
                .iter()
                .all(|l| node_labels.iter().any(|nl| nl == l))
        }

        fn stored_node_matches_filters(node: StoredNode, filters: &[PropertyFilter]) -> bool {
            if filters.is_empty() {
                return true;
            }

            for f in filters {
                let value = match node.props.get(f.key) {
                    Some(v) => v,
                    None => return false,
                };

                let ok = match f.op {
                    CompareOp::Eq => value == &f.value,
                    CompareOp::Ne => value != &f.value,

                    CompareOp::Gt => numeric_cmp(value, &f.value, |a, b| a > b),
                    CompareOp::Ge => numeric_cmp(value, &f.value, |a, b| a >= b),
                    CompareOp::Lt => numeric_cmp(value, &f.value, |a, b| a < b),
                    CompareOp::Le => numeric_cmp(value, &f.value, |a, b| a <= b),

                    CompareOp::Contains => {
                        if let (Some(lhs), Some(rhs)) = (value.as_str(), f.value.as_str()) {
                            lhs.contains(rhs)
                        } else {
                            false
                        }
                    }
                };

                if !ok {
                    return false;
                }
            }

            true
        }

        fn node_to_row(node: &StoredNode) -> QueryRow {
            QueryRow {
                values: BTreeMap::from([(
                    "n".to_string(),
                    serde_json::json!({
                        "id": node.id,
                        "labels": node.labels,
                        "props": node.props,
                    }),
                )]),
            }
        }

        // ---- 1) Determine return var and find the root NodeMatch ----

        let return_var = match q.ret {
            Return::Node(v) => v,
            // By construction for Query<M> this is always Node(..)
            // Keep this defensive for future extensions.
            //_ => return Err(GrmError::Backend("Only Return::Node is supported in execute_graph_query".into())),
        };

        // Find the NodeMatch corresponding to the returned var.
        let root_nm: NodeMatch = q
            .matches
            .iter()
            .find_map(|m| {
                if let MatchClause::Node(nm) = m {
                    if nm.var == return_var {
                        return Some(nm.clone());
                    }
                }
                None
            })
            .ok_or_else(|| {
                GrmError::Backend("GraphQuery missing root NodeMatch for return var".into())
            })?;

        // Build a lookup for any additional NodeMatch clauses by var id (end-node filters)
        let mut node_match_by_var: HashMap<VarId, NodeMatch> = HashMap::new();
        for m in &q.matches {
            if let MatchClause::Node(nm) = m {
                node_match_by_var.insert(nm.var, nm.clone());
            }
        }

        // Extract hops in the order they appear. (Your compiler emits a chain.)
        let hops: Vec<HopMatch> = q
            .matches
            .iter()
            .filter_map(|m| {
                if let MatchClause::Hop(h) = m {
                    Some(h.clone())
                } else {
                    None
                }
            })
            .collect();

        // ---- 2) Seed candidates from root NodeMatch ----
        // We represent an execution state as (root_node_id, current_node_id).
        #[derive(Clone, Copy, Debug)]
        struct Binding {
            root: i64,
            cur: i64,
        }

        let mut bindings: Vec<Binding> = Vec::new();

        if let Some(id) = root_nm.id_filter {
            if let Some(node) = self.working_copy.nodes.get(&id) {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node.clone(), &root_nm.property_filters)
                {
                    bindings.push(Binding {
                        root: node.id,
                        cur: node.id,
                    });
                }
            }
        } else {
            // Full scan by labels + filters
            for node in self.working_copy.nodes.values() {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node.clone(), &root_nm.property_filters)
                {
                    bindings.push(Binding {
                        root: node.id,
                        cur: node.id,
                    });
                }
            }
        }

        // Early out if no roots match
        if bindings.is_empty() {
            return Ok(QueryResult { rows: vec![] });
        }

        // ---- 3) Apply chained hops ----
        for hop in hops {
            let end_nm = node_match_by_var.get(&hop.end).cloned();

            let mut next: Vec<Binding> = Vec::new();

            for b in &bindings {
                // Expand relationships from current node
                for rel in self.working_copy.rels.values() {
                    // rel type check
                    if rel.rel_type != hop.rel_type {
                        continue;
                    }

                    // direction check + compute neighbor
                    let neighbor: Option<i64> = match hop.dir {
                        Direction::Out => {
                            if rel.from == b.cur {
                                Some(rel.to)
                            } else {
                                None
                            }
                        }
                        Direction::In => {
                            if rel.to == b.cur {
                                Some(rel.from)
                            } else {
                                None
                            }
                        }
                        Direction::Both => {
                            if rel.from == b.cur {
                                Some(rel.to)
                            } else if rel.to == b.cur {
                                Some(rel.from)
                            } else {
                                None
                            }
                        }
                    };

                    let Some(end_id) = neighbor else {
                        continue;
                    };

                    let Some(end_node) = self.working_copy.nodes.get(&end_id) else {
                        continue;
                    };

                    // end labels (from HopMatch)
                    if !labels_match(&end_node.labels, hop.end_labels) {
                        continue;
                    }

                    // optional end node filters (from a NodeMatch on the end var)
                    if let Some(nm) = &end_nm {
                        // if nm has id_filter set, enforce it
                        if let Some(required_id) = nm.id_filter {
                            if required_id != end_id {
                                continue;
                            }
                        }
                        // labels in NodeMatch should match too (defensive)
                        if !labels_match(&end_node.labels, nm.labels) {
                            continue;
                        }
                        if !stored_node_matches_filters(end_node.clone(), &nm.property_filters) {
                            continue;
                        }
                    }

                    next.push(Binding {
                        root: b.root,
                        cur: end_id,
                    });
                }
            }

            bindings = next;

            if bindings.is_empty() {
                return Ok(QueryResult { rows: vec![] });
            }
        }

        // ---- 4) Collect roots, stable-dedupe, apply paging ----
        let mut seen: HashSet<i64> = HashSet::new();
        let mut roots: Vec<i64> = Vec::new();

        for b in &bindings {
            if seen.insert(b.root) {
                roots.push(b.root);
            }
        }

        let roots = apply_paging(roots, q.offset, q.limit);

        // ---- 5) Emit QueryResult rows ----
        let mut rows: Vec<QueryRow> = Vec::new();
        for id in roots {
            if let Some(node) = self.working_copy.nodes.get(&id) {
                rows.push(node_to_row(node));
            }
        }

        Ok(QueryResult { rows })
    }

    /// Parse very small pseudo-Cypher commands
    fn execute_pseudo_cypher(&mut self, query: &str, params: &Value) -> Result<QueryResult> {
        let q = query.trim().to_uppercase();

        // CREATE node
        if q.starts_with("CREATE (") && q.contains("RETURN") {
            return self.create_node(params);
        }

        // UPDATE node props: MATCH (n) WHERE id(n) = $id SET n += $props RETURN n
        if q.starts_with("MATCH (N)") && q.contains("SET N +=") {
            return self.update_node(params);
        }

        // DELETE node: MATCH (n) WHERE id(n) = $id DELETE n
        if q.starts_with("MATCH (N)") && q.contains("DELETE N") {
            return self.delete_node(params);
        }

        // MATCH node by ID
        if q.starts_with("MATCH (") && q.contains("ID(N) =") {
            return self.match_node_by_id(params);
        }

        // MATCH by a property - this should be after MATCH by ID
        if q.starts_with("MATCH (N)") && q.contains("WHERE") && q.contains("RETURN N") {
            return self.match_node_by_property(params);
        }

        // MATCH outgoing relationships:
        // don't be too strict on whitespace, just look for these pieces
        if q.starts_with("MATCH (A)-[R]->(B)") && q.contains("RETURN R, B") {
            return self.match_outgoing(params);
        }

        // CREATE relationship
        if q.contains("CREATE (A)-[R") {
            return self.create_relationship(params);
        }

        Err(GrmError::Backend(format!("Unsupported query: {}", query)))
    }

    fn create_node(&mut self, params: &Value) -> Result<QueryResult> {
        let id = self.working_copy.next_node_id;
        self.working_copy.next_node_id += 1;

        let labels = params["labels"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        let props = params["props"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let node = StoredNode { id, labels, props };

        self.working_copy.nodes.insert(id, node.clone());

        Ok(QueryResult {
            rows: vec![QueryRow {
                values: BTreeMap::from([(
                    "n".to_string(),
                    serde_json::json!({
                        "id": node.id,
                        "labels": node.labels,
                        "props": node.props,
                    }),
                )]),
            }],
        })
    }

    fn match_node_by_id(&mut self, params: &Value) -> Result<QueryResult> {
        let id = params["id"]
            .as_i64()
            .ok_or_else(|| GrmError::Backend("MATCH requires id param".into()))?;

        if let Some(node) = self.working_copy.nodes.get(&id) {
            return Ok(QueryResult {
                rows: vec![QueryRow {
                    values: BTreeMap::from([(
                        "n".to_string(),
                        serde_json::json!({
                            "id": node.id,
                            "labels": node.labels,
                            "props": node.props,
                        }),
                    )]),
                }],
            });
        }

        Ok(QueryResult { rows: vec![] })
    }

    fn match_node_by_property(&mut self, params: &Value) -> Result<QueryResult> {
        let key = params["key"]
            .as_str()
            .ok_or_else(|| GrmError::Backend("MATCH-by-property requires 'key' string".into()))?;

        let value = &params["value"];

        let mut rows = vec![];

        for node in self.working_copy.nodes.values() {
            if let Some(prop) = node.props.get(key) {
                if prop == value {
                    rows.push(QueryRow {
                        values: BTreeMap::from([(
                            "n".to_string(),
                            serde_json::json!({
                                "id": node.id,
                                "labels": node.labels,
                                "props": node.props,
                            }),
                        )]),
                    });
                }
            }
        }

        Ok(QueryResult { rows })
    }

    fn create_relationship(&mut self, params: &Value) -> Result<QueryResult> {
        let from = params["from"].as_i64().unwrap();
        let to = params["to"].as_i64().unwrap();
        let rel_type = params["type"].as_str().unwrap().to_string();

        let props = params["props"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let id = self.working_copy.next_rel_id;
        self.working_copy.next_rel_id += 1;

        let rel = StoredRel {
            id,
            rel_type,
            from,
            to,
            props,
        };

        self.working_copy.rels.insert(id, rel.clone());

        Ok(QueryResult {
            rows: vec![QueryRow {
                values: BTreeMap::from([(
                    "r".to_string(),
                    serde_json::json!({
                        "id": rel.id,
                        "from": rel.from,
                        "to": rel.to,
                        "type": rel.rel_type,
                        "props": rel.props,
                    }),
                )]),
            }],
        })
    }

    fn update_node(&mut self, params: &Value) -> Result<QueryResult> {
        let id = params["id"]
            .as_i64()
            .ok_or_else(|| GrmError::Backend("UPDATE requires id param".into()))?;

        let props_obj = params["props"]
            .as_object()
            .ok_or_else(|| GrmError::Backend("UPDATE requires props object".into()))?;

        let mut result = QueryResult { rows: vec![] };

        if let Some(node) = self.working_copy.nodes.get_mut(&id) {
            // Merge props (SET n += $props semantics)
            for (k, v) in props_obj {
                node.props.insert(k.clone(), v.clone());
            }

            result.rows = vec![QueryRow {
                values: BTreeMap::from([(
                    "n".to_string(),
                    serde_json::json!({
                        "id": node.id,
                        "labels": node.labels,
                        "props": node.props,
                    }),
                )]),
            }];
        }

        Ok(result)
    }

    fn delete_node(&mut self, params: &Value) -> Result<QueryResult> {
        let id = params["id"]
            .as_i64()
            .ok_or_else(|| GrmError::Backend("DELETE requires id param".into()))?;

        self.working_copy.nodes.remove(&id);

        // Also delete relationships attached to this node
        self.working_copy
            .rels
            .retain(|_, rel| rel.from != id && rel.to != id);

        Ok(QueryResult { rows: vec![] })
    }

    fn match_outgoing(&mut self, params: &Value) -> Result<QueryResult> {
        let from = params["from"]
            .as_i64()
            .ok_or_else(|| GrmError::Backend("MATCH outgoing requires from param".into()))?;

        // optional type filter – if missing or empty, treat as wildcard
        let rel_type = params.get("type").and_then(|v| v.as_str()).unwrap_or("");

        let mut rows = Vec::new();

        for rel in self.working_copy.rels.values() {
            if rel.from == from && (rel_type.is_empty() || rel.rel_type == rel_type) {
                if let Some(node) = self.working_copy.nodes.get(&rel.to) {
                    rows.push(QueryRow {
                        values: BTreeMap::from([
                            (
                                "r".to_string(),
                                serde_json::json!({
                                    "id": rel.id,
                                    "from": rel.from,
                                    "to": rel.to,
                                    "type": rel.rel_type,
                                    "props": rel.props,
                                }),
                            ),
                            (
                                "b".to_string(),
                                serde_json::json!({
                                    "id": node.id,
                                    "labels": node.labels,
                                    "props": node.props,
                                }),
                            ),
                        ]),
                    });
                }
            }
        }

        Ok(QueryResult { rows })
    }
}

#[async_trait]
impl GraphTx for InMemoryTx {
    async fn execute_query(&mut self, query: &str, params: Value) -> Result<QueryResult> {
        self.execute_pseudo_cypher(query, &params)
    }

    async fn execute_graph(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        self.execute_graph_query(q)
    }

    async fn commit(mut self) -> Result<()> {
        let mut global = self.store.lock().unwrap();
        *global = self.working_copy.clone();
        self.committed = true;
        Ok(())
    }

    async fn rollback(mut self) -> Result<()> {
        self.committed = true;
        Ok(())
    }
}

#[async_trait]
impl GraphBackend for InMemoryBackend {
    type Tx = InMemoryTx;

    async fn execute_query(&self, query: &str, params: Value) -> Result<QueryResult> {
        let mut tx = InMemoryTx::new(self.store.clone());
        let out = tx.execute_pseudo_cypher(query, &params)?;
        tx.commit().await?;
        Ok(out)
    }

    async fn begin_tx(&self) -> Result<Self::Tx> {
        Ok(InMemoryTx::new(self.store.clone()))
    }
}
