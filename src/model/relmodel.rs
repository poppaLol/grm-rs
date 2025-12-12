use crate::{NodeModel, error::GrmError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Debug};

/// Trait for relationship models (e.g. AUTHORED).
pub trait RelModel: Sized {
    /// Relationship type, e.g. "AUTHORED"
    const TYPE: &'static str;

    type Id: Clone + Serialize + Debug + for<'de> Deserialize<'de> + From<i64> + Into<i64>;

    /// Source node
    type From: NodeModel;
    /// Target node
    type To: NodeModel;

    fn id(&self) -> &Self::Id;
    fn set_id(&mut self, id: Self::Id);

    fn to_properties(&self) -> BTreeMap<String, Value>;
    fn from_properties(id: Self::Id, props: BTreeMap<String, Value>) -> Result<Self, GrmError>;
}
