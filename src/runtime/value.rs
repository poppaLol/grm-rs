use std::cmp::Ordering;

use base64::Engine as _;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde_json::{Map, Value, json};

use crate::{CompareOp, GrmError, Result};

const TYPE_FIELD: &str = "$grm_type";
const VALUE_FIELD: &str = "value";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveKind {
    String,
    Int,
    Float,
    Bool,
    Bytes,
    Decimal,
    Date,
    DateTime,
    Duration,
    Uuid,
}

impl PrimitiveKind {
    pub fn parse_keyword(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "string" => Some(Self::String),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            "bytes" => Some(Self::Bytes),
            "decimal" => Some(Self::Decimal),
            "date" => Some(Self::Date),
            "datetime" => Some(Self::DateTime),
            "duration" => Some(Self::Duration),
            "uuid" => Some(Self::Uuid),
            _ => None,
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Bytes => "bytes",
            Self::Decimal => "decimal",
            Self::Date => "date",
            Self::DateTime => "datetime",
            Self::Duration => "duration",
            Self::Uuid => "uuid",
        }
    }
}

pub fn parse_typed_value(kind: PrimitiveKind, input: &str) -> Result<Value> {
    match kind {
        PrimitiveKind::String => Ok(Value::String(input.to_string())),
        PrimitiveKind::Int => input
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| GrmError::Constraint("expected int value".into())),
        PrimitiveKind::Float => {
            let value = input
                .trim()
                .parse::<f64>()
                .map_err(|_| GrmError::Constraint("expected finite float value".into()))?;
            finite_float_value(value)
        }
        PrimitiveKind::Bool => input
            .trim()
            .to_ascii_lowercase()
            .parse::<bool>()
            .map(Value::from)
            .map_err(|_| GrmError::Constraint("expected bool value (true/false)".into())),
        PrimitiveKind::Bytes => canonical_bytes_value(input),
        PrimitiveKind::Decimal => canonical_decimal_value(input),
        PrimitiveKind::Date => canonical_date_value(input),
        PrimitiveKind::DateTime => canonical_datetime_value(input),
        PrimitiveKind::Duration => canonical_duration_value(input),
        PrimitiveKind::Uuid => canonical_uuid_value(input),
    }
}

pub fn validate_value_for_kind(kind: PrimitiveKind, value: &Value) -> bool {
    match kind {
        PrimitiveKind::String => value.is_string(),
        PrimitiveKind::Int => value.as_i64().is_some(),
        PrimitiveKind::Float => value
            .as_number()
            .filter(|number| number.is_f64())
            .and_then(serde_json::Number::as_f64)
            .is_some_and(f64::is_finite),
        PrimitiveKind::Bool => value.is_boolean(),
        PrimitiveKind::Bytes
        | PrimitiveKind::Decimal
        | PrimitiveKind::Date
        | PrimitiveKind::DateTime
        | PrimitiveKind::Duration
        | PrimitiveKind::Uuid => typed_value_parts(value).is_some_and(|(actual, raw)| {
            actual == kind.keyword() && parse_typed_value(kind, raw).ok().as_ref() == Some(value)
        }),
    }
}

pub fn compare_typed_values(left: &Value, op: CompareOp, right: &Value) -> bool {
    if !values_have_same_kind(left, right) {
        return false;
    }
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Gt => {
            compare_typed_value_order(left, right).is_some_and(|ord| ord == Ordering::Greater)
        }
        CompareOp::Ge => {
            compare_typed_value_order(left, right).is_some_and(|ord| ord != Ordering::Less)
        }
        CompareOp::Lt => {
            compare_typed_value_order(left, right).is_some_and(|ord| ord == Ordering::Less)
        }
        CompareOp::Le => {
            compare_typed_value_order(left, right).is_some_and(|ord| ord != Ordering::Greater)
        }
        CompareOp::Contains => match (left.as_str(), right.as_str()) {
            (Some(lhs), Some(rhs)) => lhs.contains(rhs),
            _ => false,
        },
    }
}

pub fn typed_value_kind(value: &Value) -> Option<&str> {
    typed_value_parts(value).map(|(kind, _)| kind)
}

pub fn typed_value_payload(value: &Value) -> Option<&str> {
    typed_value_parts(value).map(|(_, raw)| raw)
}

fn tagged(kind: PrimitiveKind, value: String) -> Value {
    json!({ TYPE_FIELD: kind.keyword(), VALUE_FIELD: value })
}

fn typed_value_parts(value: &Value) -> Option<(&str, &str)> {
    let object = value.as_object()?;
    let kind = object.get(TYPE_FIELD)?.as_str()?;
    let raw = object.get(VALUE_FIELD)?.as_str()?;
    if object.len() == 2 {
        Some((kind, raw))
    } else {
        None
    }
}

fn finite_float_value(value: f64) -> Result<Value> {
    if !value.is_finite() {
        return Err(GrmError::Constraint("expected finite float value".into()));
    }
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| GrmError::Constraint("expected finite float value".into()))
}

fn canonical_bytes_value(input: &str) -> Result<Value> {
    let trimmed = input.trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| GrmError::Constraint("expected canonical base64 bytes value".into()))?;
    let canonical = base64::engine::general_purpose::STANDARD.encode(bytes);
    if canonical != trimmed {
        return Err(GrmError::Constraint(
            "expected canonical base64 bytes value".into(),
        ));
    }
    Ok(tagged(PrimitiveKind::Bytes, canonical))
}

fn canonical_decimal_value(input: &str) -> Result<Value> {
    let canonical = canonical_decimal(input)?;
    Ok(tagged(PrimitiveKind::Decimal, canonical))
}

fn canonical_date_value(input: &str) -> Result<Value> {
    let raw = input.trim();
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| GrmError::Constraint("expected canonical date value YYYY-MM-DD".into()))?;
    let canonical = date.format("%Y-%m-%d").to_string();
    if canonical != raw {
        return Err(GrmError::Constraint(
            "expected canonical date value YYYY-MM-DD".into(),
        ));
    }
    Ok(tagged(PrimitiveKind::Date, canonical))
}

fn canonical_datetime_value(input: &str) -> Result<Value> {
    let raw = input.trim();
    let parsed = DateTime::parse_from_rfc3339(raw).map_err(|_| {
        GrmError::Constraint("expected RFC3339 datetime with explicit offset".into())
    })?;
    let canonical = parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::AutoSi, true);
    Ok(tagged(PrimitiveKind::DateTime, canonical))
}

fn canonical_duration_value(input: &str) -> Result<Value> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(GrmError::Constraint(
            "expected canonical duration value like PT0S or -PT1.5S".into(),
        ));
    }
    let negative = raw.starts_with('-');
    let body = raw.strip_prefix('-').unwrap_or(raw);
    let seconds = body
        .strip_prefix("PT")
        .and_then(|value| value.strip_suffix('S'))
        .ok_or_else(|| {
            GrmError::Constraint("expected canonical duration value like PT0S or -PT1.5S".into())
        })?;
    let nanos = parse_decimal_seconds_to_nanos(seconds)?;
    let signed = if negative { -nanos } else { nanos };
    let canonical = format_duration_nanos(signed);
    if canonical != raw {
        return Err(GrmError::Constraint(
            "expected canonical duration value like PT0S or -PT1.5S".into(),
        ));
    }
    Ok(tagged(PrimitiveKind::Duration, canonical))
}

fn canonical_uuid_value(input: &str) -> Result<Value> {
    let raw = input.trim();
    let uuid = uuid::Uuid::parse_str(raw)
        .map_err(|_| GrmError::Constraint("expected canonical uuid value".into()))?;
    let canonical = uuid.hyphenated().to_string();
    if canonical != raw {
        return Err(GrmError::Constraint("expected canonical uuid value".into()));
    }
    Ok(tagged(PrimitiveKind::Uuid, canonical))
}

fn canonical_decimal(input: &str) -> Result<String> {
    let raw = input.trim();
    if raw.starts_with('+') {
        return Err(GrmError::Constraint(
            "expected canonical finite decimal value".into(),
        ));
    }
    let (negative, rest) = raw
        .strip_prefix('-')
        .map_or((false, raw), |rest| (true, rest));
    if rest.is_empty() || rest.contains('e') || rest.contains('E') {
        return Err(GrmError::Constraint(
            "expected canonical finite decimal value".into(),
        ));
    }
    let mut parts = rest.split('.');
    let int = parts.next().unwrap_or_default();
    let frac = parts.next();
    if parts.next().is_some()
        || int.is_empty()
        || !int.chars().all(|c| c.is_ascii_digit())
        || frac.is_some_and(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(GrmError::Constraint(
            "expected canonical finite decimal value".into(),
        ));
    }
    if int.len() > 1 && int.starts_with('0') {
        return Err(GrmError::Constraint(
            "expected canonical finite decimal value".into(),
        ));
    }
    let mut frac = frac.unwrap_or_default().trim_end_matches('0').to_string();
    let is_zero = int.chars().all(|c| c == '0') && frac.is_empty();
    if negative && is_zero {
        return Err(GrmError::Constraint(
            "expected canonical finite decimal value".into(),
        ));
    }
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(int);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    if out != raw {
        return Err(GrmError::Constraint(
            "expected canonical finite decimal value".into(),
        ));
    }
    frac.clear();
    Ok(out)
}

pub fn compare_typed_value_order(left: &Value, right: &Value) -> Option<Ordering> {
    if !values_have_same_kind(left, right) {
        return None;
    }
    if let (Some(l), Some(r)) = (left.as_i64(), right.as_i64()) {
        return Some(l.cmp(&r));
    }
    if left.as_number().is_some_and(serde_json::Number::is_f64)
        && right.as_number().is_some_and(serde_json::Number::is_f64)
        && let (Some(l), Some(r)) = (left.as_f64(), right.as_f64())
    {
        return l.partial_cmp(&r);
    }
    if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
        return Some(l.cmp(r));
    }
    if let (Some(l), Some(r)) = (left.as_bool(), right.as_bool()) {
        return Some(l.cmp(&r));
    }
    let (left_kind, left_raw) = typed_value_parts(left)?;
    let (right_kind, right_raw) = typed_value_parts(right)?;
    if left_kind != right_kind {
        return None;
    }
    match left_kind {
        "bytes" | "uuid" | "date" => Some(left_raw.cmp(right_raw)),
        "datetime" => Some(
            DateTime::parse_from_rfc3339(left_raw)
                .ok()?
                .with_timezone(&Utc)
                .cmp(
                    &DateTime::parse_from_rfc3339(right_raw)
                        .ok()?
                        .with_timezone(&Utc),
                ),
        ),
        "decimal" => compare_decimal(left_raw, right_raw),
        "duration" => Some(
            parse_duration_nanos(left_raw)
                .ok()?
                .cmp(&parse_duration_nanos(right_raw).ok()?),
        ),
        _ => None,
    }
}

fn values_have_same_kind(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::String(_), Value::String(_)) => true,
        (Value::Number(left), Value::Number(right)) => {
            (left.is_i64() && right.is_i64())
                || (left.is_u64() && right.is_u64())
                || (left.is_f64() && right.is_f64())
        }
        (Value::Object(_), Value::Object(_)) => {
            matches!((typed_value_parts(left), typed_value_parts(right)),
                (Some((left_kind, _)), Some((right_kind, _))) if left_kind == right_kind)
        }
        (Value::Array(_), Value::Array(_)) => true,
        _ => false,
    }
}

fn compare_decimal(left: &str, right: &str) -> Option<Ordering> {
    let (left_neg, left_int, left_frac) = decimal_parts(left)?;
    let (right_neg, right_int, right_frac) = decimal_parts(right)?;
    if left_neg != right_neg {
        return Some(if left_neg {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let int_ord = left_int
        .len()
        .cmp(&right_int.len())
        .then_with(|| left_int.cmp(right_int));
    let max_frac = left_frac.len().max(right_frac.len());
    let frac_ord = (0..max_frac)
        .map(|idx| {
            left_frac
                .as_bytes()
                .get(idx)
                .copied()
                .unwrap_or(b'0')
                .cmp(&right_frac.as_bytes().get(idx).copied().unwrap_or(b'0'))
        })
        .find(|ord| *ord != Ordering::Equal)
        .unwrap_or(Ordering::Equal);
    let ord = int_ord.then(frac_ord);
    Some(if left_neg { ord.reverse() } else { ord })
}

fn decimal_parts(raw: &str) -> Option<(bool, &str, &str)> {
    let (negative, rest) = raw
        .strip_prefix('-')
        .map_or((false, raw), |rest| (true, rest));
    let mut parts = rest.split('.');
    Some((negative, parts.next()?, parts.next().unwrap_or_default()))
}

fn parse_decimal_seconds_to_nanos(raw: &str) -> Result<i128> {
    if raw.is_empty() {
        return Err(GrmError::Constraint(
            "expected canonical duration value like PT0S or -PT1.5S".into(),
        ));
    }
    let mut parts = raw.split('.');
    let whole = parts.next().unwrap_or_default();
    let frac = parts.next();
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.chars().all(|c| c.is_ascii_digit())
        || frac.is_some_and(|part| {
            part.is_empty() || part.len() > 9 || !part.chars().all(|c| c.is_ascii_digit())
        })
    {
        return Err(GrmError::Constraint(
            "expected canonical duration value like PT0S or -PT1.5S".into(),
        ));
    }
    let whole = whole
        .parse::<i128>()
        .map_err(|_| GrmError::Constraint("duration seconds are outside supported range".into()))?;
    let mut frac_digits = frac.unwrap_or_default().to_string();
    while frac_digits.len() < 9 {
        frac_digits.push('0');
    }
    let frac = if frac_digits.is_empty() {
        0
    } else {
        frac_digits.parse::<i128>().map_err(|_| {
            GrmError::Constraint("duration fraction is outside supported range".into())
        })?
    };
    whole
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(frac))
        .ok_or_else(|| GrmError::Constraint("duration is outside supported range".into()))
}

fn parse_duration_nanos(raw: &str) -> Result<i128> {
    let negative = raw.starts_with('-');
    let body = raw.strip_prefix('-').unwrap_or(raw);
    let seconds = body
        .strip_prefix("PT")
        .and_then(|value| value.strip_suffix('S'))
        .ok_or_else(|| {
            GrmError::Constraint("expected canonical duration value like PT0S or -PT1.5S".into())
        })?;
    let nanos = parse_decimal_seconds_to_nanos(seconds)?;
    Ok(if negative { -nanos } else { nanos })
}

fn format_duration_nanos(nanos: i128) -> String {
    let negative = nanos < 0;
    let abs = nanos.abs();
    let whole = abs / 1_000_000_000;
    let frac = abs % 1_000_000_000;
    let mut out = String::new();
    if negative && abs != 0 {
        out.push('-');
    }
    out.push_str("PT");
    out.push_str(&whole.to_string());
    if frac != 0 {
        let mut frac = format!("{frac:09}");
        while frac.ends_with('0') {
            frac.pop();
        }
        out.push('.');
        out.push_str(&frac);
    }
    out.push('S');
    out
}

pub fn typed_json_object(kind: PrimitiveKind, value: impl Into<String>) -> Value {
    let mut object = Map::new();
    object.insert(TYPE_FIELD.into(), Value::String(kind.keyword().into()));
    object.insert(VALUE_FIELD.into(), Value::String(value.into()));
    Value::Object(object)
}
