use std::collections::BTreeMap;
use serde_json::Value;

pub type Props = BTreeMap<String, Value>;
pub type KernelNodeId = i64;
pub type KernelRelId = i64;