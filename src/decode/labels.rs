use crate::{NodeModel, StoredNode};

pub fn labels_match<M: NodeModel>(n: &StoredNode) -> bool {
    M::LABELS.iter().all(|l| n.labels.iter().any(|sl| sl == l))
}
