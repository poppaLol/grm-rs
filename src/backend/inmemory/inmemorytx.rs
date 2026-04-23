use log::trace;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::returnplan::{stored_node_to_kernel, stored_rel_to_kernel, ReturnPlan};
use crate::backend::{BackendIdType, BackendIdentity, GraphStore, GraphTx, StoredNode, StoredRel};
use crate::dsl::{
    Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, QueryResult,
    VarId,
};
use crate::dsl::numeric_cmp;
use crate::error::{GrmError, Result};
use crate::backend::GraphPersistence;
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

/// Pure function to select bindings for return - no side effects
fn select_bindings_for_return(q: &GraphQuery, bindings: Vec<Binding>) -> Vec<Binding> {
    let ret_var = q.return_var();
    let ret_kind = q.return_kind();

    // Functional deduplication using fold - collects unique bindings
    // State: (HashSet<i64>, Vec<Binding>)
    let (_ids, unique_bindings) = bindings.into_iter().fold(
        (HashSet::new(), Vec::new()),
        |(mut seen, mut out), b| {
            let id_opt = match ret_kind {
                ReturnKind::Node => b.nodes.get(&ret_var).copied(),
                ReturnKind::Rel => b.rels.get(&ret_var).copied(),
            };

            if let Some(id) = id_opt {
                if seen.insert(id) {
                    out.push(b);
                }
            }

            (seen, out)
        },
    );

    // Apply paging to rows/bindings (stateful operations)
    let off = q.offset.unwrap_or(0);
    let mut result = unique_bindings;

    if off >= result.len() {
        return vec![];
    }

    result.drain(0..off);

    if let Some(lim) = q.limit {
        result.truncate(lim);
    }

    result
}

/// Pure function to emit rows from bindings - no side effects
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

    pub fn snapshot_nodes(&self) -> Vec<StoredNode> {
        self.store
            .lock()
            .unwrap()
            .nodes
            .values()
            .cloned()
            .collect()
    }

    pub fn snapshot_relationships(&self) -> Vec<StoredRel> {
        self.store
            .lock()
            .unwrap()
            .rels
            .values()
            .cloned()
            .collect()
    }

    pub fn snapshot_store(&self) -> GraphStore {
        self.store.lock().unwrap().clone_store()
    }

    pub fn replace_store(&self, store: GraphStore) {
        let mut current = self.store.lock().unwrap();
        *current = store;
    }

    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let store = self.store.lock().unwrap().clone_store();
        store.save_to_file(path)
    }

    pub fn load_from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let store = GraphStore::load_from_file(path)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    pub fn save_to_binary_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let store = self.store.lock().unwrap().clone_store();
        store.save_to_binary_file(path)
    }

    pub fn load_from_binary_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let store = GraphStore::load_from_binary_file(path)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }
}

impl GraphPersistence for InMemoryBackend {
    fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        self.save_to_file(path)
    }

    fn load_from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::load_from_file(path)
    }

    fn save_to_binary_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        self.save_to_binary_file(path)
    }

    fn load_from_binary_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::load_from_binary_file(path)
    }
}

impl BackendIdentity for InMemoryBackend {
    fn node_id_type(&self) -> BackendIdType {
        BackendIdType::Int64
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

    /// Pure function to seed bindings from roots - no mutation of self
    fn seed_roots(tx: &InMemoryTx, root_nm: &NodeMatch) -> Vec<Binding> {
        let mut bindings = Vec::new();

        if let Some(id) = root_nm.id_filter {
            if let Some(node) = tx.working_copy.nodes.get(&id) {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node, &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(root_nm.var, node.id));
                }
            }
        } else {
            for node in tx.working_copy.nodes.values() {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(node, &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(root_nm.var, node.id));
                }
            }
        }

        bindings
    }

    /// Traverse multiple hops - returns new bindings without mutating self
    async fn traverse_hops(
        &mut self,
        bindings: Vec<Binding>,
        ctx: &ExecCtx,
    ) -> Result<Vec<Binding>> {
        // If no hops, return bindings unchanged
        if ctx.hops.is_empty() {
            return Ok(bindings);
        }

        let mut current = bindings;
        for hop in &ctx.hops {
            let mut next_bindings = Vec::new();
            for binding in current {
                let next = Self::traverse_single_hop(self, binding, hop).await?;
                next_bindings.extend(next);
            }

            if next_bindings.is_empty() {
                return Ok(Vec::new());
            }

            current = next_bindings;
        }

        Ok(current)
    }

    /// Pure function for single-hop traversal logic - no state mutation
    /// Takes pairs of (rel, end_node) as input for pure computation
    fn traverse_single_hop_pure(
        hop: &HopMatch,
        binding: &Binding,
        pairs: Vec<(StoredRel, StoredNode)>,
    ) -> Vec<Binding> {
        let mut results = Vec::new();

        for (rel, end_node) in pairs {
            // existing checks stay exactly as-is
            if !labels_match(&end_node.labels, hop.end_labels) {
                continue;
            }

            results.push(Self::create_next_binding(
                binding.root,
                binding.cur,
                &binding.rels,
                &binding.nodes,
                rel,
                end_node.id,
                hop,
            ));
        }

        results
    }

    /// Single-hop traversal - returns new bindings
    async fn traverse_single_hop(
        &mut self,
        binding: Binding,
        hop: &HopMatch,
    ) -> Result<Vec<Binding>> {
        let rel_type = hop.rel_type.map(|t| t as &str);

        let pairs = match hop.dir {
            Direction::Out => self.outgoing(binding.cur, rel_type).await?,
            Direction::In => self.incoming(binding.cur, rel_type).await?,
            Direction::Both => self.both(binding.cur, rel_type).await?,
        };

        Ok(Self::traverse_single_hop_pure(hop, &binding, pairs))
    }

    /// Builder pattern for new bindings - combines existing values with new ones
    fn create_next_binding(
        root: i64,
        _cur: i64,
        rels: &HashMap<VarId, i64>,
        nodes: &HashMap<VarId, i64>,
        rel: StoredRel,
        end_node_id: i64,
        hop: &HopMatch,
    ) -> Binding {
        // Collect existing values into new HashMaps
        let mut new_rels = HashMap::new();
        let mut new_nodes = HashMap::new();

        for (k, v) in rels {
            new_rels.insert(*k, *v);
        }

        for (k, v) in nodes {
            new_nodes.insert(*k, *v);
        }

        // Add new values
        new_rels.insert(hop.rel_var, rel.id);
        new_nodes.insert(hop.end, end_node_id);

        Binding {
            root,
            cur: end_node_id,
            rels: new_rels,
            nodes: new_nodes,
        }
    }

    pub async fn execute_graph_query(&mut self, q: &GraphQuery) -> Result<QueryResult> {
        // ---- 1) Build the execution context
        let ctx = ExecCtx::build(q)?;

        // ---- 2) Seed candidates from *root* NodeMatch ----
        // Execution state as (root_node_id, current_node_id).
        let bindings = Self::seed_roots(self, &ctx.root_nm);
        trace!("inmemory.exec: seeded {} bindings (root)", bindings.len());

        if bindings.is_empty() {
            return Ok(QueryResult { rows: vec![] });
        }

        // ---- 3) Apply chained hops ----
        let bindings = Self::traverse_hops(self, bindings, &ctx).await?;
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
