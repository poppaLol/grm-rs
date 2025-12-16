use crate::dsl::Props;

#[derive(Debug, Clone)]
pub struct StoredNode {
    pub id: i64,
    pub labels: Vec<String>,
    pub props: Props
}