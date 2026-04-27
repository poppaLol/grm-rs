use crate::dsl::Props;
use crate::error::GrmError;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Trait for graph node models (e.g. User, Post).
pub trait NodeModel: Sized {
    /// Labels for this node type (e.g. ["User"] in Neo4j).
    const LABELS: &'static [&'static str];

    /// The ID type in Rust (e.g. uuid::Uuid, i64).
    type Id: Clone + Serialize + Debug + for<'de> Deserialize<'de> + From<i64> + Into<i64>;

    fn id(&self) -> &Self::Id;
    fn set_id(&mut self, id: Self::Id);

    /// Properties excluding the ID.
    fn to_properties(&self) -> Props;

    /// Build from ID + properties.
    fn from_properties(id: Self::Id, props: Props) -> Result<Self, GrmError>;
}
