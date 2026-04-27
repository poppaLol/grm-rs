use std::collections::BTreeMap;

use serde_json::Value;

use crate::dsl::{CompareOp, PropertyFilter};

pub fn numeric_cmp<F>(a: &Value, b: &Value, cmp: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (a.as_f64(), b.as_f64()) {
        (Some(la), Some(rb)) => cmp(la, rb),
        _ => false,
    }
}

/// Evaluate filters against a raw properties map.
/// This is the core semantics function used by both repo and backend.
pub fn props_match_filters(props: &BTreeMap<String, Value>, filters: &[PropertyFilter]) -> bool {
    if filters.is_empty() {
        return true;
    }

    for f in filters {
        let value = match props.get(f.key) {
            Some(v) => v,
            None => return false,
        };

        let ok = match f.op {
            CompareOp::Eq => value == &f.value,
            CompareOp::Ne => value != &f.value,

            CompareOp::Gt => numeric_cmp(value, &f.value, |a, b| a > b),
            CompareOp::Ge => numeric_cmp(value, &f.value, |a, b| a >= b),
            CompareOp::Lt => numeric_cmp(value, &f.value, |a, b| a < b),
            CompareOp::Le => numeric_cmp(value, &f.value, |a, b| a <= b),

            CompareOp::Contains => {
                if let (Some(lhs), Some(rhs)) = (value.as_str(), f.value.as_str()) {
                    lhs.contains(rhs)
                } else {
                    false
                }
            }
        };

        if !ok {
            return false;
        }
    }

    true
}
