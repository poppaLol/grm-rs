use crate::Props;

#[derive(Debug, Clone)]
pub struct StoredRel {
    pub id: i64,
    pub rel_type: String,
    pub from: i64,
    pub to: i64,
    pub props: Props,
}