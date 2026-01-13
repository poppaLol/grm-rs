use std::collections::HashSet;

use crate::{
    GraphQuery, StoredNode, StoredRel,
    backend::inmemory::inmemorytx::Binding,
    dsl::{KernelValue, NodeValue, RelValue, Return, VarId},
};

pub fn stored_node_to_kernel(node: &StoredNode) -> KernelValue {
    KernelValue::Node(NodeValue {
        id: node.id,
        labels: node.labels.clone(),
        props: node.props.clone(),
    })
}

pub fn stored_rel_to_kernel(rel: &StoredRel) -> KernelValue {
    KernelValue::Rel(RelValue {
        id: rel.id,
        ty: rel.rel_type.clone(),
        from: rel.from,
        to: rel.to,
        props: rel.props.clone(),
    })
}

#[allow(dead_code)]
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
                        if seen.insert(*rel) {
                            out.push(*rel);
                        }
                    }
                }
            }
        }

        out
    }
}
