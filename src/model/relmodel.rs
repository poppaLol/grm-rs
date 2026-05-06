use crate::{NodeModel, error::Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Debug};

/// Trait for relationship models (e.g. AUTHORED).
pub trait RelModel: Sized {
    /// Relationship type, e.g. "AUTHORED"
    const TYPE: &'static str;

    type Id: Clone + Serialize + Debug + for<'de> Deserialize<'de> + From<i64> + Into<i64>;

    /// Source node ID type (for full node model support)
    type From: NodeModel;
    /// Target node ID type (for full node model support)
    type To: NodeModel;

    /// Get source endpoint ID (may be None if not set via create_between)
    #[allow(clippy::wrong_self_convention)]
    fn from_id(&self) -> Option<i64> {
        panic!(
            "from_id is not available for this RelModel - use RelRepository::create_between instead"
        )
    }
    /// Get target endpoint ID (may be None if not set via create_between)
    fn to_id(&self) -> Option<i64> {
        panic!(
            "to_id is not available for this RelModel - use RelRepository::create_between instead"
        )
    }

    /// Set source endpoint ID (must be implemented manually or use macros)
    fn set_from(&mut self, _from_id: i64) {}
    /// Set target endpoint ID (must be implemented manually or use macros)
    fn set_to(&mut self, _to_id: i64) {}

    fn id(&self) -> &Self::Id;
    fn set_id(&mut self, id: Self::Id);

    fn to_properties(&self) -> BTreeMap<String, Value>;

    fn from_parts(
        id: Self::Id,
        from: <Self::From as NodeModel>::Id,
        to: <Self::To as NodeModel>::Id,
        props: BTreeMap<String, Value>,
    ) -> Result<Self>;
}
