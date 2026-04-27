use super::NodePattern;
use crate::{dsl::graph::ReturnMode, model::NodeModel};

/// The kind of query being constructed.
///
/// At this stage we only support a simple "match nodes" query.
/// Later we can add variants for:
/// - pattern matches with relationships
/// - create / update / delete
/// - aggregates, etc.
#[derive(Debug, Clone)]
pub enum QueryKind<N: NodeModel> {
    /// MATCH (n:Label { ... }) WHERE ...
    MatchNode {
        pattern: NodePattern<N>,
        limit: Option<usize>,
        offset: Option<usize>,
    },
}

/// A strongly-typed query rooted in a single node type `N`.
///
/// This is intentionally small and simple for now. It’s just a thin
/// wrapper over `QueryKind`, which you’ll later compile into the
/// in-memory backend’s internal query representation.
#[derive(Debug, Clone)]
pub struct Query<N: NodeModel> {
    pub kind: QueryKind<N>,
    pub return_mode: ReturnMode,
}

impl<N: NodeModel> Query<N> {
    /// Construct a `MATCH` query from a `NodePattern<N>`.
    ///
    /// Example:
    ///   let pattern = NodePattern::<User>::new().alias("u");
    ///   let q = Query::matching(pattern).limit(10);
    pub fn matching(pattern: NodePattern<N>) -> Self {
        Self {
            kind: QueryKind::MatchNode {
                pattern,
                limit: None,
                offset: None,
            },
            return_mode: ReturnMode::Root,
        }
    }

    /// Set a LIMIT on the query.
    pub fn limit(mut self, n: usize) -> Self {
        match &mut self.kind {
            QueryKind::MatchNode { limit, .. } => {
                *limit = Some(n);
            }
        }
        self
    }

    /// Set an OFFSET on the query.
    pub fn offset(mut self, n: usize) -> Self {
        match &mut self.kind {
            QueryKind::MatchNode { offset, .. } => {
                *offset = Some(n);
            }
        }
        self
    }

    /// Access the underlying node pattern (if this is a MatchNode).
    ///
    /// This will be useful in the backend adapter:
    /// you can inspect filters, labels, id, etc.
    pub fn node_pattern(&self) -> &NodePattern<N> {
        match &self.kind {
            QueryKind::MatchNode { pattern, .. } => pattern,
        }
    }

    /// Mutable access to the underlying node pattern.
    pub fn node_pattern_mut(&mut self) -> &mut NodePattern<N> {
        match &mut self.kind {
            QueryKind::MatchNode { pattern, .. } => pattern,
        }
    }

    /// Get the current limit (if any).
    pub fn limit_value(&self) -> Option<usize> {
        match &self.kind {
            QueryKind::MatchNode { limit, .. } => *limit,
        }
    }

    /// Get the current offset (if any).
    pub fn offset_value(&self) -> Option<usize> {
        match &self.kind {
            QueryKind::MatchNode { offset, .. } => *offset,
        }
    }

    pub fn return_end(mut self) -> Self {
        self.return_mode = ReturnMode::End;
        self
    }

    // optional symmetry:
    pub fn return_root(mut self) -> Self {
        self.return_mode = ReturnMode::Root;
        self
    }

    // get a relationship
    pub fn return_rel(mut self) -> Self {
        self.return_mode = ReturnMode::Rel;
        self
    }
}
