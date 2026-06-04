use log::trace;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::returnplan::{ReturnPlan, stored_node_to_kernel, stored_rel_to_kernel};
use crate::backend::GraphPersistence;
use crate::backend::{BackendIdType, BackendIdentity, GraphStore, GraphTx, StoredNode, StoredRel};
use crate::dsl::numeric_cmp;
use crate::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, QueryResult, VarId};
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

fn base_node_candidate_ids(
    store: &mut GraphStore,
    labels: &'static [&'static str],
    filters: &[PropertyFilter],
) -> Vec<i64> {
    let eq_filters = filters
        .iter()
        .filter(|filter| {
            filter.op == CompareOp::Eq && GraphStore::property_value_is_indexable(&filter.value)
        })
        .collect::<Vec<_>>();

    let mut best: Option<Vec<i64>> = None;
    if labels.is_empty() {
        for filter in eq_filters {
            keep_smallest(
                &mut best,
                store.node_ids_by_property(filter.key, &filter.value),
            );
        }
    } else {
        for label in labels {
            for filter in &eq_filters {
                keep_smallest(
                    &mut best,
                    store.node_ids_by_label_property(label, filter.key, &filter.value),
                );
            }
        }
        if best.is_none() {
            for label in labels {
                keep_smallest(&mut best, store.node_ids_by_label(label));
            }
        }
    }

    best.unwrap_or_else(|| store.nodes.keys().copied().collect())
}

fn keep_smallest(best: &mut Option<Vec<i64>>, candidate: Vec<i64>) {
    if best
        .as_ref()
        .map(|current| candidate.len() < current.len())
        .unwrap_or(true)
    {
        *best = Some(candidate);
    }
}

/// Pure function to select bindings for return - no side effects
fn select_bindings_for_return(q: &GraphQuery, bindings: Vec<Binding>) -> Vec<Binding> {
    let ret_var = q.return_var();
    let ret_kind = q.return_kind();

    // Functional deduplication using fold - collects unique bindings
    // State: (HashSet<i64>, Vec<Binding>)
    let (_ids, unique_bindings) =
        bindings
            .into_iter()
            .fold((HashSet::new(), Vec::new()), |(mut seen, mut out), b| {
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
            });

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
            if let Some(node) = tx.visible_node(id) {
                values.insert(var, stored_node_to_kernel(&node));
            }
        }

        // rel vars
        for (var, id) in b.rels {
            if let Some(rel) = tx.visible_rel(id) {
                values.insert(var, stored_rel_to_kernel(&rel));
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
                    node_match_by_var.insert(nm.var, nm.clone());
                }
                MatchClause::Hop(h) => {
                    hops.push(h.clone());
                }
            }
        }

        Ok(Self { root_nm, hops })
    }
}

#[derive(Clone)]
pub struct InMemoryBackend {
    pub store: Arc<Mutex<GraphStore>>,
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(GraphStore::default())),
        }
    }

    pub fn snapshot_nodes(&self) -> Vec<StoredNode> {
        self.store.lock().unwrap().nodes.values().cloned().collect()
    }

    pub fn snapshot_nodes_filtered(
        &self,
        label: &str,
        id: Option<i64>,
        property: Option<(&str, &serde_json::Value)>,
    ) -> Vec<StoredNode> {
        let mut store = self.store.lock().unwrap();

        if let Some(id) = id {
            return store
                .nodes
                .get(&id)
                .filter(|node| node.labels.iter().any(|node_label| node_label == label))
                .cloned()
                .into_iter()
                .collect();
        }

        let node_ids = match property {
            Some((key, value)) => store.node_ids_by_label_property(label, key, value),
            None => store.node_ids_by_label(label),
        };

        node_ids
            .into_iter()
            .filter_map(|id| store.nodes.get(&id))
            .cloned()
            .collect()
    }

    pub fn snapshot_relationships(&self) -> Vec<StoredRel> {
        self.store.lock().unwrap().rels.values().cloned().collect()
    }

    pub fn snapshot_relationships_filtered(
        &self,
        rel_type: &str,
        id: Option<i64>,
        from: Option<i64>,
        to: Option<i64>,
    ) -> Vec<StoredRel> {
        let store = self.store.lock().unwrap();

        if let Some(id) = id {
            return store
                .rels
                .get(&id)
                .filter(|rel| {
                    rel.rel_type == rel_type
                        && from.map(|from| rel.from == from).unwrap_or(true)
                        && to.map(|to| rel.to == to).unwrap_or(true)
                })
                .cloned()
                .into_iter()
                .collect();
        }

        let rel_ids = match (from, to) {
            (Some(from), Some(to)) => {
                let outgoing = store.outgoing_relationship_ids(from, Some(rel_type));
                let incoming = store.incoming_relationship_ids(to, Some(rel_type));
                if outgoing.len() <= incoming.len() {
                    outgoing
                } else {
                    incoming
                }
            }
            (Some(from), None) => store.outgoing_relationship_ids(from, Some(rel_type)),
            (None, Some(to)) => store.incoming_relationship_ids(to, Some(rel_type)),
            (None, None) => store.relationship_ids_by_type(rel_type),
        };

        rel_ids
            .into_iter()
            .filter_map(|id| store.rels.get(&id))
            .filter(|rel| {
                rel.rel_type == rel_type
                    && from.map(|from| rel.from == from).unwrap_or(true)
                    && to.map(|to| rel.to == to).unwrap_or(true)
            })
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
    pub working_copy: Option<GraphStore>,
    pub delta: TxDelta,
    pub committed: bool,
}

#[derive(Default)]
pub struct TxDelta {
    pub nodes: BTreeMap<i64, StoredNode>,
    pub deleted_nodes: BTreeSet<i64>,
    pub rels: BTreeMap<i64, StoredRel>,
    pub deleted_rels: BTreeSet<i64>,
}

impl InMemoryTx {
    pub fn new(store: Arc<Mutex<GraphStore>>) -> Self {
        Self {
            store,
            working_copy: None,
            delta: TxDelta::default(),
            committed: false,
        }
    }

    pub fn materialize_working_copy(&mut self) {
        if self.working_copy.is_some() {
            return;
        }

        let mut snapshot = self.store.lock().unwrap().clone_store();
        self.apply_delta_to(&mut snapshot);
        self.working_copy = Some(snapshot);
    }

    pub fn materialized_store(&self) -> &GraphStore {
        self.working_copy
            .as_ref()
            .expect("in-memory transaction working copy must be materialized")
    }

    pub fn materialized_store_mut(&mut self) -> &mut GraphStore {
        self.materialize_working_copy();
        self.working_copy
            .as_mut()
            .expect("in-memory transaction working copy must be materialized")
    }

    pub fn allocate_node_id(&self) -> i64 {
        let mut global = self.store.lock().unwrap();
        let id = global.next_node_id;
        global.next_node_id += 1;
        id
    }

    pub fn allocate_rel_id(&self) -> i64 {
        let mut global = self.store.lock().unwrap();
        let id = global.next_rel_id;
        global.next_rel_id += 1;
        id
    }

    pub fn apply_delta_to(&self, store: &mut GraphStore) {
        for id in &self.delta.deleted_nodes {
            store.remove_node(*id);
        }
        for id in &self.delta.deleted_rels {
            store.remove_relationship(*id);
        }
        for (id, node) in &self.delta.nodes {
            store.next_node_id = store.next_node_id.max(id + 1);
            store.insert_node(*id, node.clone());
        }
        for (id, rel) in &self.delta.rels {
            store.next_rel_id = store.next_rel_id.max(id + 1);
            store.insert_relationship(*id, rel.clone());
        }
    }

    pub fn visible_node(&self, id: i64) -> Option<StoredNode> {
        if let Some(store) = &self.working_copy {
            return store.nodes.get(&id).cloned();
        }
        if self.delta.deleted_nodes.contains(&id) {
            return None;
        }
        self.delta
            .nodes
            .get(&id)
            .cloned()
            .or_else(|| self.store.lock().unwrap().nodes.get(&id).cloned())
    }

    pub fn visible_rel(&self, id: i64) -> Option<StoredRel> {
        if let Some(store) = &self.working_copy {
            return store.rels.get(&id).cloned();
        }
        if self.delta.deleted_rels.contains(&id) {
            return None;
        }
        let rel = self
            .delta
            .rels
            .get(&id)
            .cloned()
            .or_else(|| self.store.lock().unwrap().rels.get(&id).cloned())?;

        if self.delta.deleted_nodes.contains(&rel.from)
            || self.delta.deleted_nodes.contains(&rel.to)
        {
            return None;
        }

        Some(rel)
    }

    pub fn visible_nodes_by_property(&self, key: &str, value: &Value) -> Vec<StoredNode> {
        if let Some(store) = &self.working_copy {
            return store
                .nodes
                .values()
                .filter(|n| n.props.get(key).map(|v| v == value).unwrap_or(false))
                .cloned()
                .collect();
        }

        let mut nodes = {
            let mut store = self.store.lock().unwrap();
            store
                .node_ids_by_property(key, value)
                .into_iter()
                .filter_map(|id| store.nodes.get(&id))
                .filter(|node| {
                    !self.delta.deleted_nodes.contains(&node.id)
                        && !self.delta.nodes.contains_key(&node.id)
                        && node.props.get(key).is_some_and(|v| v == value)
                })
                .cloned()
                .collect::<Vec<_>>()
        };

        nodes.extend(
            self.delta
                .nodes
                .values()
                .filter(|node| node.props.get(key).map(|v| v == value).unwrap_or(false))
                .cloned(),
        );
        nodes
    }

    fn base_nodes_matching(
        &self,
        labels: &'static [&'static str],
        filters: &[PropertyFilter],
    ) -> Vec<StoredNode> {
        let mut store = self.store.lock().unwrap();
        let candidate_ids = base_node_candidate_ids(&mut store, labels, filters);

        candidate_ids
            .into_iter()
            .filter_map(|id| store.nodes.get(&id))
            .filter(|node| {
                !self.delta.deleted_nodes.contains(&node.id)
                    && !self.delta.nodes.contains_key(&node.id)
                    && labels_match(&node.labels, labels)
                    && stored_node_matches_filters(node, filters)
            })
            .cloned()
            .collect()
    }

    pub fn visible_nodes_matching(
        &self,
        labels: &'static [&'static str],
        filters: &[PropertyFilter],
    ) -> Vec<StoredNode> {
        if let Some(store) = &self.working_copy {
            return store
                .nodes
                .values()
                .filter(|node| {
                    labels_match(&node.labels, labels) && stored_node_matches_filters(node, filters)
                })
                .cloned()
                .collect();
        }

        let mut nodes = self.base_nodes_matching(labels, filters);

        nodes.extend(
            self.delta
                .nodes
                .values()
                .filter(|node| {
                    labels_match(&node.labels, labels) && stored_node_matches_filters(node, filters)
                })
                .cloned(),
        );
        nodes
    }

    pub fn visible_neighbor_pairs(
        &self,
        node: i64,
        rel_type: Option<&str>,
        dir: Direction,
    ) -> Vec<(StoredRel, StoredNode)> {
        if let Some(store) = &self.working_copy {
            return materialized_neighbor_pairs(store, node, rel_type, dir);
        }

        if self.delta.deleted_nodes.contains(&node) {
            return Vec::new();
        }

        let mut seen_rel_ids = BTreeSet::new();
        let mut pairs = Vec::new();
        let base_rel_ids = {
            let store = self.store.lock().unwrap();
            if !self.delta.nodes.contains_key(&node) && !store.nodes.contains_key(&node) {
                return Vec::new();
            }
            match dir {
                Direction::Out => store.outgoing_relationship_ids(node, rel_type),
                Direction::In => store.incoming_relationship_ids(node, rel_type),
                Direction::Both => {
                    let mut ids = store.outgoing_relationship_ids(node, rel_type);
                    ids.extend(store.incoming_relationship_ids(node, rel_type));
                    ids
                }
            }
        };

        for rel_id in base_rel_ids {
            if !seen_rel_ids.insert(rel_id) {
                continue;
            }
            if let Some((rel, neighbor)) = self.visible_neighbor_pair(node, rel_id, dir) {
                pairs.push((rel, neighbor));
            }
        }

        for rel in self.delta.rels.values() {
            if !rel_matches_neighbor(rel, node, rel_type, dir) || !seen_rel_ids.insert(rel.id) {
                continue;
            }
            if let Some(neighbor) = neighbor_id(rel, node, dir).and_then(|id| self.visible_node(id))
            {
                pairs.push((rel.clone(), neighbor));
            }
        }

        pairs
    }

    fn visible_neighbor_pair(
        &self,
        node: i64,
        rel_id: i64,
        dir: Direction,
    ) -> Option<(StoredRel, StoredNode)> {
        let rel = self.visible_rel(rel_id)?;
        let neighbor = neighbor_id(&rel, node, dir).and_then(|id| self.visible_node(id))?;
        Some((rel, neighbor))
    }

    pub fn visible_related_rel_ids(&self, node: i64) -> BTreeSet<i64> {
        if let Some(store) = &self.working_copy {
            return store
                .rels
                .values()
                .filter(|rel| rel.from == node || rel.to == node)
                .map(|rel| rel.id)
                .collect();
        }

        if self.delta.deleted_nodes.contains(&node) {
            return BTreeSet::new();
        }

        let mut ids = {
            let store = self.store.lock().unwrap();
            if !self.delta.nodes.contains_key(&node) && !store.nodes.contains_key(&node) {
                return BTreeSet::new();
            }
            let mut ids = store.outgoing_relationship_ids(node, None);
            ids.extend(store.incoming_relationship_ids(node, None));
            ids.into_iter().collect::<BTreeSet<_>>()
        };
        ids.retain(|id| self.visible_rel(*id).is_some());
        ids.extend(
            self.delta
                .rels
                .values()
                .filter(|rel| rel.from == node || rel.to == node)
                .map(|rel| rel.id),
        );
        ids
    }

    /// Pure function to seed bindings from roots - no mutation of self
    fn seed_roots(tx: &InMemoryTx, root_nm: &NodeMatch) -> Vec<Binding> {
        let mut bindings = Vec::new();

        if let Some(id) = root_nm.id_filter {
            if let Some(node) = tx.visible_node(id) {
                if labels_match(&node.labels, root_nm.labels)
                    && stored_node_matches_filters(&node, &root_nm.property_filters)
                {
                    bindings.push(Binding::new_root(root_nm.var, node.id));
                }
            }
        } else {
            for node in tx.visible_nodes_matching(root_nm.labels, &root_nm.property_filters) {
                bindings.push(Binding::new_root(root_nm.var, node.id));
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

fn materialized_neighbor_pairs(
    store: &GraphStore,
    node: i64,
    rel_type: Option<&str>,
    dir: Direction,
) -> Vec<(StoredRel, StoredNode)> {
    let mut seen_rel_ids = BTreeSet::new();
    let rel_ids = match dir {
        Direction::Out => store.outgoing_relationship_ids(node, rel_type),
        Direction::In => store.incoming_relationship_ids(node, rel_type),
        Direction::Both => {
            let mut ids = store.outgoing_relationship_ids(node, rel_type);
            ids.extend(store.incoming_relationship_ids(node, rel_type));
            ids
        }
    };

    let mut pairs = Vec::new();
    for rel_id in rel_ids {
        if !seen_rel_ids.insert(rel_id) {
            continue;
        }
        let Some(rel) = store.rels.get(&rel_id) else {
            continue;
        };
        if let Some(neighbor) = neighbor_id(rel, node, dir).and_then(|id| store.nodes.get(&id)) {
            pairs.push((rel.clone(), neighbor.clone()));
        }
    }
    pairs
}

fn rel_matches_neighbor(
    rel: &StoredRel,
    node: i64,
    rel_type: Option<&str>,
    dir: Direction,
) -> bool {
    if rel_type.is_some_and(|ty| rel.rel_type != ty) {
        return false;
    }
    match dir {
        Direction::Out => rel.from == node,
        Direction::In => rel.to == node,
        Direction::Both => rel.from == node || rel.to == node,
    }
}

fn neighbor_id(rel: &StoredRel, node: i64, dir: Direction) -> Option<i64> {
    match dir {
        Direction::Out if rel.from == node => Some(rel.to),
        Direction::In if rel.to == node => Some(rel.from),
        Direction::Both if rel.from == node => Some(rel.to),
        Direction::Both if rel.to == node => Some(rel.from),
        _ => None,
    }
}
