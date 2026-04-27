use std::marker::PhantomData;

use crate::PropertyFilter;
use crate::dsl::Direction;
use crate::model::{NodeModel, RelModel};

#[derive(Debug, Clone)]
pub struct TraversalStep {
    pub dir: super::graph::Direction,
    pub rel_type: Option<&'static str>,
    pub end_labels: &'static [&'static str], // from NodeModel::LABELS
    pub end_filters: Vec<super::PropertyFilter>,
    pub end_alias: Option<String>,
}

pub struct TraversalBuilder<N: NodeModel, R: RelModel> {
    pat: NodePattern<N>,
    dir: super::graph::Direction,
    _r: PhantomData<R>,
}

pub struct TraversalBuilderAny<N: NodeModel> {
    pat: NodePattern<N>,
    dir: Direction,
}

impl<N: NodeModel, R: RelModel> TraversalBuilder<N, R> {
    pub fn to<M: NodeModel>(mut self) -> NodePattern<N> {
        self.pat.traversals.push(TraversalStep {
            dir: self.dir,
            rel_type: Some(R::TYPE),
            end_labels: M::LABELS,
            end_filters: vec![],
            end_alias: None,
        });
        self.pat
    }

    /// Convenience: allow filtering the end node inline
    pub fn to_where<M: NodeModel>(
        mut self,
        build: impl FnOnce(NodePattern<M>) -> NodePattern<M>,
    ) -> NodePattern<N> {
        let end_pat = build(NodePattern::<M>::new());
        self.pat.traversals.push(TraversalStep {
            dir: self.dir,
            rel_type: Some(R::TYPE),
            end_labels: M::LABELS,
            end_filters: end_pat.property_filters,
            end_alias: end_pat.alias,
        });
        self.pat
    }
}

impl<N: NodeModel> TraversalBuilderAny<N> {
    pub fn to<M: NodeModel>(mut self) -> NodePattern<N> {
        self.pat.traversals.push(TraversalStep {
            rel_type: None, // <-- KEY DIFFERENCE
            dir: self.dir,
            end_labels: M::LABELS,
            end_filters: vec![],
            end_alias: None,
        });
        self.pat
    }

    pub fn to_where<M: NodeModel>(
        mut self,
        f: impl FnOnce(NodePattern<M>) -> NodePattern<M>,
    ) -> NodePattern<N> {
        let np = f(NodePattern::<M>::new());
        self.pat.traversals.push(TraversalStep {
            rel_type: None,
            dir: self.dir,
            end_labels: M::LABELS,
            end_filters: np.property_filters,
            end_alias: np.alias,
        });
        self.pat
    }
}

/// A typed representation of a node pattern in a query.
///
///   (alias:Label1:Label2 { property_filters... })
#[derive(Debug, Clone)]
pub struct NodePattern<N: NodeModel> {
    /// All labels of the node type (Neo4j-style).
    pub labels: &'static [&'static str],

    /// Optional alias (like `u` in MATCH (u:User)).
    pub alias: Option<String>,

    /// Optional concrete ID filter (e.g. MATCH (u:User {id: 123})).
    pub id: Option<N::Id>,

    /// Property filters applied to this node.
    pub property_filters: Vec<PropertyFilter>,

    pub traversals: Vec<TraversalStep>,

    _marker: PhantomData<N>,
}

impl<N: NodeModel> NodePattern<N> {
    /// Construct a pattern for this node model, using its declared labels.
    pub fn new() -> Self {
        Self {
            labels: N::LABELS,
            alias: None,
            id: None,
            property_filters: Vec::new(),
            traversals: vec![],
            _marker: PhantomData,
        }
    }

    /// Override the alias (e.g. "u" → MATCH (u:User)).
    pub fn alias<S: Into<String>>(mut self, alias: S) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Constrain this pattern by ID.
    pub fn with_id(mut self, id: N::Id) -> Self {
        self.id = Some(id);
        self
    }

    /// Add a single property filter.
    pub fn filter(mut self, filter: PropertyFilter) -> Self {
        self.property_filters.push(filter);
        self
    }

    /// Add multiple property filters in one go.
    pub fn filters<I>(mut self, filters: I) -> Self
    where
        I: IntoIterator<Item = PropertyFilter>,
    {
        self.property_filters.extend(filters);
        self
    }

    /// Convenience: primary label (first in LABELS).
    /// Panics if LABELS is empty – your derive macro should guarantee it's not.
    pub fn primary_label(&self) -> &'static str {
        self.labels.first().copied().unwrap_or("")
    }

    pub fn out<R: RelModel>(self) -> TraversalBuilder<N, R> {
        TraversalBuilder {
            pat: self,
            dir: super::graph::Direction::Out,
            _r: PhantomData,
        }
    }

    pub fn incoming<R: RelModel>(self) -> TraversalBuilder<N, R> {
        TraversalBuilder {
            pat: self,
            dir: super::graph::Direction::In,
            _r: PhantomData,
        }
    }

    pub fn both<R: RelModel>(self) -> TraversalBuilder<N, R> {
        TraversalBuilder {
            pat: self,
            dir: super::graph::Direction::Both,
            _r: PhantomData,
        }
    }

    pub fn out_any(self) -> TraversalBuilderAny<N> {
        TraversalBuilderAny {
            pat: self,
            dir: Direction::Out,
        }
    }

    pub fn incoming_any(self) -> TraversalBuilderAny<N> {
        TraversalBuilderAny {
            pat: self,
            dir: Direction::In,
        }
    }

    pub fn both_any(self) -> TraversalBuilderAny<N> {
        TraversalBuilderAny {
            pat: self,
            dir: Direction::Both,
        }
    }
}
