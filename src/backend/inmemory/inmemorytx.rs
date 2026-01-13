use log::trace;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::returnplan::{stored_node_to_kernel, stored_rel_to_kernel, ReturnPlan};
use crate::backend::{GraphStore, GraphTx, StoredNode};
use crate::dsl::{
    Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, QueryResult,
    VarId,
};
use crate::dsl::numeric_cmp;
use crate::error::{GrmError, Result};
use crate::{CompareOp, PropertyFilter, QueryRow, ReturnKind};

fn labels_match(node_labels: &[String], required: &'static [&'static str]) -> bool {
    required
        .iter()
        .all(|l| node_labels.iter().any(|nl| nl == l))
}

fn stored_node_matches_filters(node: &StoredNode, filters: &[PropertyFilter]) -> bool {
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

fn select_bindings_for_return(q: &GraphQuery, bindings: Vec<Binding>) -> Vec<Binding> {
    let ret_var = q.return_var();
    let ret_kind = q.return_kind();

    let mut seen = HashSet::<i64>::new();
    let mut out = Vec::new();

    for b in bindings {
        let id_opt = match ret_kind {
            ReturnKind::Node => b.nodes.get(&ret_var).copied(),
            ReturnKind::Rel => b.rels.get(&ret_var).copied(),
        };

        let Some(id) = id_opt else { continue };

        if seen.insert(id) {
            out.push(b);
        }
    }

    // Apply paging to rows/bindings
    let off = q.offset.unwrap_or(0);
    if off >= out.len() {
        return vec![];
    }
    out.drain(0..off);

    if let Some(lim) = q.limit {
        out.truncate(lim);
    }

    out
}

fn emit_rows_from_bindings(tx: &InMemoryTx, bindings: Vec<Binding>) -> Vec<QueryRow> {
    let mut rows = Vec::with_capacity(bindings.len());

    for b in bindings {
        let mut values = std::collections::BTreeMap::new();

        // node vars (root + hop end vars)
        for (var, id) in b.nodes {
            if let Some(node) = tx.working_copy.nodes.get(&id) {
                values.insert(var, stored_node_to_kernel(node));
            }
        }

        // rel vars
        for (var, id) in b.rels {
            if let Some(rel) = tx.working_copy.rels.get(&id) {
                values.insert(var, stored_rel_to_kernel(rel));
            }
        }

        rows.push(QueryRow { values });
    }

    rows
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub root: i64,
    pub cur: i64,
    pub rels: HashMap<VarId, i64>,
    pub nodes: HashMap<VarId, i64>,
}

impl Binding {
    pub fn new_root(root_var: VarId, root_id: i64) -> Self {
        let mut nodes = HashMap::new();
        nodes.insert(root_var, root_id);

        Self {
            root: root_id,
            cur: root_id,
            rels: HashMap::new(),
            nodes,
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
                    && stored_node_matches_filters(node, &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(root_nm.var, node.id));
                }
            }
        } else {
            for node in self.working_copy.nodes.values() {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node, &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(root_nm.var, node.id));
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
                        if !stored_node_matches_filters(&end_node, &nm.property_filters) {
                            continue;
                        }
                    }

                    let mut rels = b.rels.clone();
                    rels.insert(hop.rel_var, rel.id);
                    let mut nodes = b.nodes.clone();
                    nodes.insert(hop.end, end_node.id);

                    next.push(Binding {
                        root: b.root,
                        cur: end_node.id,
                        rels,
                        nodes,
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
        //let ids = apply_paging(ids, q.offset, q.limit);

        let selected = select_bindings_for_return(q, bindings);

        // ---- 5) Emit full binding rows ----
        let rows = emit_rows_from_bindings(self, selected);
        Ok(QueryResult { rows })
    }
}
