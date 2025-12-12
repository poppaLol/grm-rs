use std::marker::PhantomData;

use crate::NodeModel;
use crate::PropertyFilter;

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
}

