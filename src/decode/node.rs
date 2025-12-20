use crate::{
    decode::DecodeFromRow,
    dsl::KernelValue,
    error::{GrmError, Result},
    model::NodeModel,
    GraphQuery,
    QueryRow,
};

impl<M: NodeModel> DecodeFromRow for M {
    fn decode(gq: &GraphQuery, row: &QueryRow) -> Result<Self> {
        let v = row
            .get_returned(gq)
            .ok_or_else(|| GrmError::Backend("execute_graph row missing return var".into()))?;

        let node = match v {
            KernelValue::Node(n) => n,
            _ => return Err(GrmError::Backend("expected node return value".into())),
        };

        M::from_properties(node.id.into(), node.props.clone())
    }
}
