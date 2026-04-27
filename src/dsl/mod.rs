mod compare;
mod eval;
mod graph;
mod kernel;
mod nodepattern;
mod paging;
mod property;
mod query;
mod result;

pub use compare::CompareOp;
pub use eval::{numeric_cmp, props_match_filters};
pub use graph::{
    Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, NodeValue, RelValue, Return,
    ReturnKind, Value as KernelValue, VarGen, VarId,
};
pub use kernel::{KernelNodeId, KernelRelId, Props};
pub use nodepattern::NodePattern;
pub use paging::apply_paging;
pub use property::{Property, PropertyFilter};
pub use query::{Query, QueryKind};
pub use result::{QueryResult, QueryRow};
