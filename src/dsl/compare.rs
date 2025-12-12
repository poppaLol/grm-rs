/// Comparison operator for filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
}
