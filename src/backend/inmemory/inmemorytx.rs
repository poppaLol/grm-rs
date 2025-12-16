use log::trace;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::backend::inmemory::returnplan::ReturnPlan;
use crate::backend::{GraphStore, GraphTx, StoredNode};
use crate::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, QueryResult, VarId};
use crate::dsl::{apply_paging, numeric_cmp};
use crate::error::{GrmError, Result};
use crate::{CompareOp, PropertyFilter};

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

#[derive(Clone, Debug)]
pub struct Binding {
    pub root: i64,
    pub cur: i64,
    pub rels: HashMap<VarId, i64>,
}

impl Binding {
    fn new_root(id: i64) -> Self {
        Self {
            root: id,
            cur: id,
            rels: HashMap::new(),
        }
    }
}

struct ExecCtx {
    root_nm: NodeMatch,
    node_match_by_var: HashMap<VarId, NodeMatch>,
    hops: Vec<HopMatch>,
}

impl ExecCtx {
    fn build(q: &GraphQuery) -> Result<Self> {
        let root_nm = q
            .matches
            .iter()
            .find_map(|m| match m {
                MatchClause::Node(nm) => Some(nm.clone()),
                _ => None,
            })
            .ok_or_else(|| GrmError::Backend("GraphQuery missing root NodeMatch".into()))?;

        let mut node_match_by_var = HashMap::new();
        let mut hops = Vec::new();

        for m in &q.matches {
            match m {
                MatchClause::Node(nm) => {
                    node_match_by_var.insert(nm.var.clone(), nm.clone());
                }
                MatchClause::Hop(h) => {
                    hops.push(h.clone());
                }
            }
        }

        Ok(Self {
            root_nm,
            node_match_by_var,
            hops,
        })
    }
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

    fn seed_roots(&self, root_nm: &NodeMatch) -> Vec<Binding> {
        let mut bindings = Vec::new();

        if let Some(id) = root_nm.id_filter {
            if let Some(node) = self.working_copy.nodes.get(&id) {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node.clone(), &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(node.id));
                }
            }
        } else {
            for node in self.working_copy.nodes.values() {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node.clone(), &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(node.id));
                }
            }
        }

        bindings
    }

    async fn traverse_hops(
        &mut self,
        mut bindings: Vec<Binding>,
        ctx: &ExecCtx,
    ) -> Result<Vec<Binding>> {
        for hop in &ctx.hops {
            let end_nm = ctx.node_match_by_var.get(&hop.end).cloned();
            let mut next = Vec::new();

            for b in &bindings {
                let rel_type = hop.rel_type.map(|t| t as &str);

                let pairs = match hop.dir {
                    Direction::Out => self.outgoing(b.cur, rel_type).await?,
                    Direction::In => self.incoming(b.cur, rel_type).await?,
                    Direction::Both => self.both(b.cur, rel_type).await?,
                };

                for (rel, end_node) in pairs {
                    // existing checks stay exactly as-is
                    if !labels_match(&end_node.labels, hop.end_labels) {
                        continue;
                    }

                    if let Some(nm) = &end_nm {
                        if let Some(id) = nm.id_filter {
                            if id != end_node.id {
                                continue;
                            }
                        }
                        if !labels_match(&end_node.labels, nm.labels) {
                            continue;
                        }
                        if !stored_node_matches_filters(end_node.clone(), &nm.property_filters) {
                            continue;
                        }
                    }

                    let mut rels = b.rels.clone();
                    rels.insert(hop.rel_var.clone(), rel.id);

                    next.push(Binding {
                        root: b.root,
                        cur: end_node.id,
                        rels,
                    });
                }
            }

            bindings = next;
            if bindings.is_empty() {
                break;
            }
        }

        Ok(bindings)
    }

    pub async fn execute_graph_query(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        // ---- 1) Build the execution context
        let ctx = ExecCtx::build(q)?;

        // ---- 2) Seed candidates from *root* NodeMatch ----
        // Execution state as (root_node_id, current_node_id).
        let mut bindings = self.seed_roots(&ctx.root_nm);
        trace!("inmemory.exec: seeded {} bindings (root)", bindings.len());

        if bindings.is_empty() {
            return Ok(QueryResult { rows: vec![] });
        }

        // ---- 3) Apply chained hops ----
        bindings = self.traverse_hops(bindings, &ctx).await?;
        trace!("inmemory.exec: {} bindings after hops", bindings.len());

        if bindings.is_empty() {
            return Ok(QueryResult { rows: vec![] });
        }

        // ---- 4) Collect returned ids, stable-dedupe, apply paging ----
        let plan = ReturnPlan::new(q, &ctx.root_nm.var);

        let ids = plan.collect_ids(&bindings);
        trace!("inmemory.exec: {} returned ids ({:?})", ids.len(), q.ret);
        let ids = apply_paging(ids, q.offset, q.limit);

        let rows = plan.emit_rows(self, ids);
        Ok(QueryResult { rows })
    }
}
