use std::marker::PhantomData;

use serde_json::Value;

use crate::CompareOp;

/// A single property predicate on a node.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyFilter {
    pub key: &'static str,
    pub op: CompareOp,
    pub value: Value,
}

/// Typed property handle, parameterised by node type `N` and value `T`.
#[derive(Debug, Clone, Copy)]
pub struct Property<N, T> {
    pub key: &'static str,
    _n: PhantomData<N>,
    _t: PhantomData<T>,
}

impl<N, T> Property<N, T> {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key,
            _n: PhantomData,
            _t: PhantomData,
        }
    }

    pub fn eq<V>(self, v: V) -> PropertyFilter
    where
        V: Into<Value>,
    {
        PropertyFilter {
            key: self.key,
            op: CompareOp::Eq,
            value: v.into(),
        }
    }

    pub fn ne<V>(self, v: V) -> PropertyFilter
    where
        V: Into<Value>,
    {
        PropertyFilter {
            key: self.key,
            op: CompareOp::Ne,
            value: v.into(),
        }
    }

    pub fn contains<S>(self, s: S) -> PropertyFilter
    where
        S: Into<String>,
    {
        PropertyFilter {
            key: self.key,
            op: CompareOp::Contains,
            value: Value::String(s.into()),
        }
    }

    pub fn gt<V>(self, v: V) -> PropertyFilter
    where
        V: Into<Value>,
    {
        PropertyFilter {
            key: self.key,
            op: CompareOp::Gt,
            value: v.into(),
        }
    }

    pub fn lt<V>(self, v: V) -> PropertyFilter
    where
        V: Into<Value>,
    {
        PropertyFilter {
            key: self.key,
            op: CompareOp::Lt,
            value: v.into(),
        }
    }
}
