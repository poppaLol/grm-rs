use crate::dsl::Props;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNode {
    pub id: i64,
    pub labels: Vec<String>,
    pub props: Props,
}
