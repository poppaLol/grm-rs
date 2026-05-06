use crate::{
    GraphQuery, QueryRow,
    decode::{DecodeFromRow, decoderow::DecodeFromRowAt},
    dsl::{KernelValue, VarId},
    error::{GrmError, Result},
    model::NodeModel,
};

impl<M: NodeModel> DecodeFromRow for M {
    fn decode(gq: &GraphQuery, row: &QueryRow) -> Result<Self> {
        decode_node_from_row::<M>(gq, row)
    }
}

pub fn decode_node_from_row<M: NodeModel>(gq: &GraphQuery, row: &QueryRow) -> Result<M> {
    let v = row
        .get_returned(gq)
        .ok_or_else(|| GrmError::Backend("execute_graph row missing return var".into()))?;

    let node = match v {
        KernelValue::Node(n) => n,
        _ => return Err(GrmError::Backend("expected node return value".into())),
    };

    M::from_properties(node.id.into(), node.props.clone())
}

#[allow(dead_code)]
pub fn decode_node_at<M>(gq: &GraphQuery, row: &QueryRow, var: VarId) -> Result<M>
where
    M: NodeModel + DecodeFromRowAt,
{
    M::decode_at(gq, row, var)
}
