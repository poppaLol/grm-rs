use std::collections::{BTreeMap, HashSet};

use crate::{
    GraphQuery, StoredNode, StoredRel, backend::inmemory::{InMemoryTx, inmemorytx::Binding}, dsl::{KernelValue, NodeValue, QueryRow, RelValue, Return, VarId}
};

fn node_to_row(var: VarId, node: &StoredNode) -> QueryRow {
    QueryRow {
        values: BTreeMap::from([(
            var,
            KernelValue::Node(NodeValue {
                id: node.id,
                labels: node.labels.clone(),
                props: node.props.clone(),
            }),
        )]),
    }
}

fn rel_to_row(var: VarId, rel: &StoredRel) -> QueryRow {
    QueryRow {
        values: BTreeMap::from([(
            var,
            KernelValue::Rel(RelValue {
                id: rel.id,
                ty: rel.rel_type.clone(),
                from: rel.from,
                to: rel.to,
                props: rel.props.clone(),
            }),
        )]),
    }
}

pub enum ReturnPlan {
    Node { var: VarId, return_is_root: bool },
    Rel { var: VarId },
}

impl ReturnPlan {
    pub fn new(q: &GraphQuery, root_var: &VarId) -> Self {
        match q.ret {
            Return::Node(v) => ReturnPlan::Node {
                var: v,
                return_is_root: v == *root_var,
            },
            Return::Rel(v) => ReturnPlan::Rel { var: v },
        }
    }

    pub fn collect_ids(&self, bindings: &[Binding]) -> Vec<i64> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();

        match self {
            ReturnPlan::Node { return_is_root, .. } => {
                for b in bindings {
                    let id = if *return_is_root { b.root } else { b.cur };
                    if seen.insert(id) {
                        out.push(id);
                    }
                }
            }
            ReturnPlan::Rel { var } => {
                for b in bindings {
                    if let Some(rel) = b.rels.get(var) {
                        if seen.insert(rel.id) {
                            out.push(rel.id);
                        }
                    }
                }
            }
        }

        out
    }

    pub fn emit_rows(&self, tx: &InMemoryTx, ids: Vec<i64>) -> Vec<QueryRow> {
        let mut rows = Vec::with_capacity(ids.len());

        match self {
            ReturnPlan::Node { var, .. } => {
                for id in ids {
                    if let Some(node) = tx.working_copy.nodes.get(&id) {
                        rows.push(node_to_row(var.clone(), node));
                    }
                }
            }
            ReturnPlan::Rel { var } => {
                for id in ids {
                    if let Some(rel) = tx.working_copy.rels.get(&id) {
                        rows.push(rel_to_row(var.clone(), rel));
                    }
                }
            }
        }

        rows
    }
}
