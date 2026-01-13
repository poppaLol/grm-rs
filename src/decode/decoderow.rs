use crate::{
    GraphQuery, GrmError, dsl::{KernelValue, QueryRow}, error::Result
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