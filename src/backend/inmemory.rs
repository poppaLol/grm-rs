use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use crate::backend::{
    GraphBackend, GraphStore, GraphTx, QueryResult, QueryRow, StoredNode, StoredRel,
};
use crate::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use crate::dsl::{apply_paging, numeric_cmp};
use crate::error::{GrmError, Result};
use crate::{CompareOp, PropertyFilter};

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

    async fn execute_graph_query(&mut self, q: &GraphQuery) -> Result<QueryResult> {
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
                // Expand neighbors from current node using GraphTx helpers
                let rel_type: Option<&str> = hop.rel_type.map(|t| t as &str);

                let pairs: Vec<(StoredRel, StoredNode)> = match hop.dir {
                    Direction::Out => self.outgoing(b.cur, rel_type).await?,
                    Direction::In => self.incoming(b.cur, rel_type).await?,
                    Direction::Both => self.both(b.cur, rel_type).await?,
                };

                for (_rel, end_node) in pairs {
                    let end_id = end_node.id;

                    // end labels (from HopMatch)
                    if !labels_match(&end_node.labels, hop.end_labels) {
                        continue;
                    }

                    // optional end node filters (from a NodeMatch on the end var)
                    if let Some(nm) = &end_nm {
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
}

#[async_trait]
impl GraphTx for InMemoryTx {
    async fn execute_query(&mut self, _query: &str, _params: Value) -> Result<QueryResult> {
        Err(GrmError::Backend(
            "InMemory backend does not support string queries; use typed APIs".into(),
        ))
    }

    async fn execute_graph(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        self.execute_graph_query(q).await
    }

    async fn create_node(
        &mut self,
        labels: Vec<String>,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredNode> {
        let id = self.working_copy.next_node_id;
        self.working_copy.next_node_id += 1;

        let node = StoredNode { id, labels, props };
        self.working_copy.nodes.insert(id, node.clone());
        Ok(node)
    }

    async fn update_node(
        &mut self,
        id: i64,
        props: BTreeMap<String, Value>,
    ) -> Result<Option<StoredNode>> {
        if let Some(node) = self.working_copy.nodes.get_mut(&id) {
            for (k, v) in props {
                node.props.insert(k, v);
            }
            return Ok(Some(node.clone()));
        }
        Ok(None)
    }

    async fn delete_node(&mut self, id: i64) -> Result<()> {
        self.working_copy.nodes.remove(&id);
        self.working_copy
            .rels
            .retain(|_, rel| rel.from != id && rel.to != id);
        Ok(())
    }

    async fn find_node_by_id(&mut self, id: i64) -> Result<Option<StoredNode>> {
        Ok(self.working_copy.nodes.get(&id).cloned())
    }

    async fn find_nodes_by_property(
        &mut self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<StoredNode>> {
        Ok(self
            .working_copy
            .nodes
            .values()
            .filter(|n| n.props.get(key).map(|v| v == value).unwrap_or(false))
            .cloned()
            .collect())
    }

    async fn create_relationship(
        &mut self,
        from: i64,
        to: i64,
        rel_type: String,
        props: BTreeMap<String, Value>,
    ) -> Result<StoredRel> {
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
        Ok(rel)
    }

    async fn outgoing(
        &mut self,
        from: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let mut out = Vec::new();
        for rel in self.working_copy.rels.values() {
            if rel.from != from {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.to) {
                out.push((rel.clone(), n.clone()));
            }
        }
        Ok(out)
    }

    async fn incoming(
        &mut self,
        to: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let mut out = Vec::new();
        for rel in self.working_copy.rels.values() {
            if rel.to != to {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.from) {
                out.push((rel.clone(), n.clone()));
            }
        }
        Ok(out)
    }

    async fn both(
        &mut self,
        node: i64,
        rel_type: Option<&str>,
    ) -> Result<Vec<(StoredRel, StoredNode)>> {
        let mut out = Vec::new();
        let mut seen_rel_ids = std::collections::BTreeSet::new();

        // outgoing neighbors
        for rel in self.working_copy.rels.values() {
            if rel.from != node {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.to) {
                if seen_rel_ids.insert(rel.id) {
                    out.push((rel.clone(), n.clone()));
                }
            }
        }

        // incoming neighbors
        for rel in self.working_copy.rels.values() {
            if rel.to != node {
                continue;
            }
            if let Some(t) = rel_type {
                if rel.rel_type != t {
                    continue;
                }
            }
            if let Some(n) = self.working_copy.nodes.get(&rel.from) {
                if seen_rel_ids.insert(rel.id) {
                    out.push((rel.clone(), n.clone()));
                }
            }
        }

        Ok(out)
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

    async fn execute_query(&self, _query: &str, _params: Value) -> Result<QueryResult> {
        Err(GrmError::Backend(
            "InMemoryBackend does not support string queries; use execute_graph (typed)".into(),
        ))
    }

    async fn begin_tx(&self) -> Result<Self::Tx> {
        Ok(InMemoryTx::new(self.store.clone()))
    }

    // Optional: implement directly (otherwise the trait default uses begin_tx + commit)
    async fn execute_graph(&self, q: &GraphQuery) -> Result<QueryResult> {
        let mut tx = InMemoryTx::new(self.store.clone());
        tx.execute_graph_query(q).await
    }
}
