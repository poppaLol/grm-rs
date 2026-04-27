use serde_json::Value;
use std::collections::BTreeMap;

pub type Props = BTreeMap<String, Value>;
pub type KernelNodeId = i64;
pub type KernelRelId = i64;
