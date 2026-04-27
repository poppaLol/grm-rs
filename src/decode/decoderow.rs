use crate::{
    GraphQuery, GrmError, NodeModel,
    dsl::{KernelValue, QueryRow, VarId},
    error::Result,
};

/// Decode a typed value from a single kernel `QueryRow`.
///
/// This is intentionally backend-agnostic: it only depends on the kernel result
/// contract (`QueryRow`) produced by any backend implementing `GraphTx`.
pub trait DecodeFromRow: Sized {
    fn decode(gq: &GraphQuery, row: &QueryRow) -> Result<Self>;
}

/// Decode just the single returned value when the row contains exactly one value.
/// Useful for quick tests / kernel-level assertions.
impl DecodeFromRow for KernelValue {
    fn decode(_gq: &GraphQuery, row: &QueryRow) -> Result<Self> {
        match row.values.len() {
            1 => Ok(row.values.values().next().unwrap().clone()),
            0 => Err(GrmError::Mapping("row had no values".into())),
            n => Err(GrmError::Mapping(format!(
                "row had {n} values; KernelValue::decode expects exactly 1"
            ))),
        }
    }
}

#[allow(dead_code)]
pub trait DecodeFromRowAt: Sized {
    fn decode_at(gq: &GraphQuery, row: &QueryRow, var: VarId) -> Result<Self>;
}

impl<M: NodeModel> DecodeFromRowAt for M {
    fn decode_at(_gq: &GraphQuery, row: &QueryRow, var: VarId) -> Result<Self> {
        let v = row
            .get(&var)
            .ok_or_else(|| GrmError::Backend("row missing var".into()))?;

        let node = match v {
            KernelValue::Node(n) => n,
            _ => return Err(GrmError::Backend("expected node at var".into())),
        };

        M::from_properties(node.id.into(), node.props.clone())
    }
}
