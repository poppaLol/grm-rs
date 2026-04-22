use crate::{
    GraphQuery,
    decode::decoderow::DecodeFromRowAt,
    dsl::{KernelValue, QueryRow, VarId},
    error::{GrmError, Result},
    model::RelModel,
};

pub fn decode_rel_from_row<R: RelModel>(gq: &GraphQuery, row: &QueryRow) -> Result<R> {
    let v = row
        .get_returned(gq)
        .ok_or_else(|| GrmError::Backend("execute_graph row missing return var".into()))?;

    let rel = match v {
        KernelValue::Rel(r) => r,
        _ => return Err(GrmError::Backend("expected rel return value".into())),
    };

    R::from_parts(rel.id.into(), rel.from.into(), rel.to.into(), rel.props.clone())
}

#[allow(dead_code)]
pub fn decode_rel_at<R: RelModel>(gq: &GraphQuery, row: &QueryRow, var: VarId) -> Result<R>
where
    R: DecodeFromRowAt,
{
    R::decode_at(gq, row, var)
}