use crate::{
    dsl::KernelValue,
    error::{GrmError, Result},
    model::RelModel,
    GraphQuery,
    dsl::QueryRow,
};

pub fn decode_rel_from_row<R: RelModel>(gq: &GraphQuery, row: &QueryRow) -> Result<R> {
    let v = row
        .get_returned(gq)
        .ok_or_else(|| GrmError::Backend("execute_graph row missing return var".into()))?;

    let rel = match v {
        KernelValue::Rel(r) => r,
        _ => return Err(GrmError::Backend("expected rel return value".into())),
    };

    R::from_properties(rel.id.into(), rel.props.clone())
}
