use std::marker::PhantomData;

use crate::{
    dsl::{GraphQuery, KernelValue, QueryRow, VarId},
    error::{GrmError, Result},
    model::{NodeModel, RelModel},
};

pub struct PickNode<M> {
    var: VarId,
    _pd: PhantomData<fn() -> M>,
}

pub struct PickRel<R> {
    var: VarId,
    _pd: PhantomData<fn() -> R>,
}

impl<M> PickNode<M> {
    pub fn new(var: VarId) -> Self {
        Self {
            var,
            _pd: PhantomData,
        }
    }
}

impl<R> PickRel<R> {
    pub fn new(var: VarId) -> Self {
        Self {
            var,
            _pd: PhantomData,
        }
    }
}

// Ergonomic constructors so call sites look like: node::<User>(u_var)
pub fn node<M: NodeModel>(var: VarId) -> PickNode<M> {
    PickNode::new(var)
}

pub fn rel<R: RelModel>(var: VarId) -> PickRel<R> {
    PickRel::new(var)
}

/// A "shape descriptor" that knows how to decode itself from a single QueryRow.
pub trait ResultShape {
    type Out;
    fn decode(&self, gq: &GraphQuery, row: &QueryRow) -> Result<Self::Out>;
}

impl<M: NodeModel> ResultShape for PickNode<M> {
    type Out = M;

    fn decode(&self, _gq: &GraphQuery, row: &QueryRow) -> Result<M> {
        let v = row
            .get(&self.var)
            .ok_or_else(|| GrmError::Backend("row missing var".into()))?;

        let node = match v {
            KernelValue::Node(n) => n,
            _ => return Err(GrmError::Backend("expected node at var".into())),
        };

        M::from_properties(node.id.into(), node.props.clone())
    }
}

impl<R: RelModel> ResultShape for PickRel<R> {
    type Out = R;

    fn decode(&self, _gq: &GraphQuery, row: &QueryRow) -> Result<R> {
        let v = row
            .get(&self.var)
            .ok_or_else(|| GrmError::Backend("row missing var".into()))?;

        let relv = match v {
            KernelValue::Rel(r) => r,
            _ => return Err(GrmError::Backend("expected rel at var".into())),
        };

        R::from_parts(
            relv.id.into(),
            relv.from.into(),
            relv.to.into(),
            relv.props.clone(),
        )
    }
}

// Tuple composition (start with 2 + 3; add more if you like)
impl<A, B> ResultShape for (A, B)
where
    A: ResultShape,
    B: ResultShape,
{
    type Out = (A::Out, B::Out);

    fn decode(&self, gq: &GraphQuery, row: &QueryRow) -> Result<Self::Out> {
        Ok((self.0.decode(gq, row)?, self.1.decode(gq, row)?))
    }
}

impl<A, B, C> ResultShape for (A, B, C)
where
    A: ResultShape,
    B: ResultShape,
    C: ResultShape,
{
    type Out = (A::Out, B::Out, C::Out);

    fn decode(&self, gq: &GraphQuery, row: &QueryRow) -> Result<Self::Out> {
        Ok((
            self.0.decode(gq, row)?,
            self.1.decode(gq, row)?,
            self.2.decode(gq, row)?,
        ))
    }
}
