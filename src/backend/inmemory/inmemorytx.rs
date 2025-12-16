use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::backend::inmemory::returnplan::ReturnPlan;
use crate::backend::{GraphStore, GraphTx, StoredNode, StoredRel};
use crate::dsl::{
    Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, QueryResult,
    VarId,
};
use crate::dsl::{apply_paging, numeric_cmp};
use crate::error::{GrmError, Result};
use crate::{CompareOp, PropertyFilter};

#[derive(Clone, Debug)]
pub struct Binding {
    pub root: i64,
    pub cur: i64,
    pub rels: HashMap<VarId, StoredRel>,
}

#[derive(Clone)]
pub struct InMemoryBackend {
    pub store: Arc<Mutex<GraphStore>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(GraphStore::default())),
        }
    }
}

pub struct InMemoryTx {
    pub store: Arc<Mutex<GraphStore>>,
    pub working_copy: GraphStore,
    pub committed: bool,
}

impl InMemoryTx {
    pub fn new(store: Arc<Mutex<GraphStore>>) -> Self {
        let snapshot = store.lock().unwrap().clone_store();
        Self {
            store,
            working_copy: snapshot,
            committed: false,
        }
    }

    pub async fn execute_graph_query(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        // ---- Helpers (kept local to avoid plumbing) ----

        fn labels_match(node_labels: &[String], required: &'static [&'static str]) -> bool {
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

        // Root is the *first* NodeMatch (compiler emits it first).
        let root_nm: NodeMatch = q
            .matches
            .iter()
            .find_map(|m| match m {
                MatchClause::Node(nm) => Some(nm.clone()),
                _ => None,
            })
            .ok_or_else(|| GrmError::Backend("GraphQuery missing root NodeMatch".into()))?;

        // Build a lookup for NodeMatch clauses by var id (end-node filters, etc.)
        let mut node_match_by_var: HashMap<VarId, NodeMatch> = HashMap::new();
        for m in &q.matches {
            if let MatchClause::Node(nm) = m {
                node_match_by_var.insert(nm.var.clone(), nm.clone());
            }
        }

        // Extract hops in the order they appear. (Your compiler emits a chain.)
        let hops: Vec<HopMatch> = q
            .matches
            .iter()
            .filter_map(|m| match m {
                MatchClause::Hop(h) => Some(h.clone()),
                _ => None,
            })
            .collect();

        // ---- 2) Seed candidates from *root* NodeMatch ----
        // Execution state as (root_node_id, current_node_id).
        let mut bindings: Vec<Binding> = Vec::new();

        if let Some(id) = root_nm.id_filter {
            if let Some(node) = self.working_copy.nodes.get(&id) {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node.clone(), &root_nm.property_filters)
                {
                    bindings.push(Binding {
                        root: node.id,
                        cur: node.id,
                        rels: HashMap::new(),
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
                        rels: HashMap::new(),
                    });
                }
            }
        }

        if bindings.is_empty() {
            return Ok(QueryResult { rows: vec![] });
        }

        // ---- 3) Apply chained hops ----
        for hop in hops {
            let end_nm = node_match_by_var.get(&hop.end).cloned();

            let mut next: Vec<Binding> = Vec::new();

            for b in &bindings {
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

                    let mut rels = b.rels.clone();
                    rels.insert(hop.rel_var.clone(), _rel);

                    next.push(Binding {
                        root: b.root,
                        cur: end_id,
                        rels,
                    });
                }
            }

            bindings = next;

            if bindings.is_empty() {
                return Ok(QueryResult { rows: vec![] });
            }
        }

        // ---- 4) Collect returned ids, stable-dedupe, apply paging ----
        let plan = ReturnPlan::new(q, &root_nm.var);

        let ids = plan.collect_ids(&bindings);
        let ids = apply_paging(ids, q.offset, q.limit);

        let rows = plan.emit_rows(self, ids);
        Ok(QueryResult { rows })
    }
}
