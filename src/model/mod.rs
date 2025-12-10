mod props;

use crate::error::GrmError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Trait for graph node models (e.g. User, Post).
pub trait NodeModel: Sized {
    /// Labels for this node type (e.g. ["User"] in Neo4j).
    const LABELS: &'static [&'static str];

    /// The ID type in Rust (e.g. uuid::Uuid, i64).
    type Id: Clone + Serialize + for<'de> Deserialize<'de> + From<i64> + Into<i64>;

    fn id(&self) -> &Self::Id;
    fn set_id(&mut self, id: Self::Id);

    /// Properties excluding the ID.
    fn to_properties(&self) -> BTreeMap<String, Value>;

    /// Build from ID + properties.
    fn from_properties(id: Self::Id, props: BTreeMap<String, Value>) -> Result<Self, GrmError>;
}

/// Trait for relationship models (e.g. AUTHORED).
pub trait RelModel: Sized {
    /// Relationship type, e.g. "AUTHORED"
    const TYPE: &'static str;

    type Id: Clone + Serialize + for<'de> Deserialize<'de> + From<i64> + Into<i64>;

    /// Source node
    type From: NodeModel;
    /// Target node
    type To: NodeModel;

    fn id(&self) -> &Self::Id;
    fn set_id(&mut self, id: Self::Id);

    fn to_properties(&self) -> BTreeMap<String, Value>;
    fn from_properties(id: Self::Id, props: BTreeMap<String, Value>) -> Result<Self, GrmError>;
}

pub use props::{from_props, to_props};