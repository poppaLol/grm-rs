use crate::Query;
use crate::{NodeModel, RelModel};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarId(pub u32);

#[derive(Debug, Clone)]
pub struct NodeVar<N: NodeModel> {
    pub id: VarId,
    pub alias: Option<String>,
    _n: PhantomData<N>,
}

#[derive(Debug, Clone)]
pub struct RelVar<R: RelModel> {
    pub id: VarId,
    pub alias: Option<String>,
    _r: PhantomData<R>,
}

#[derive(Debug, Default)]
pub struct VarGen {
    next: u32,
}

impl VarGen {
    pub fn fresh(&mut self) -> VarId {
        let id = VarId(self.next);
        self.next += 1;
        id
    }

    pub fn node<N: NodeModel>(&mut self, alias: Option<String>) -> NodeVar<N> {
        NodeVar {
            id: self.fresh(),
            alias,
            _n: PhantomData,
        }
    }

    pub fn rel<R: RelModel>(&mut self, alias: Option<String>) -> RelVar<R> {
        RelVar {
            id: self.fresh(),
            alias,
            _r: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
    Both,
}

#[derive(Debug, Clone)]
pub enum MatchClause {
    Node(NodeMatch),
    Hop(HopMatch),
}

#[derive(Debug, Clone)]
pub struct NodeMatch {
    pub var: VarId,
    pub labels: &'static [&'static str],
    pub id_filter: Option<i64>, // kernel-level id; you can keep typed id outside
    pub property_filters: Vec<crate::dsl::PropertyFilter>,
}

#[derive(Debug, Clone)]
pub struct HopMatch {
    pub start: VarId,
    pub rel_type: &'static str,
    pub rel_var: VarId,
    pub dir: Direction,
    pub end: VarId,
    pub end_labels: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub enum Return {
    Node(VarId),
    // future: Rel(VarId), Tuple(Vec<VarId>), Subgraph, Path, etc.
}

#[derive(Debug, Clone)]
pub struct GraphQuery {
    pub matches: Vec<MatchClause>,
    pub where_: Vec<crate::dsl::PropertyFilter>, // AND semantics for now
    pub ret: Return,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl<N: NodeModel> Query<N> {
    pub fn compile_to_graph(&self) -> crate::dsl::graph::GraphQuery {
        let mut vg = VarGen::default();

        // root var
        let root = vg.node::<N>(self.node_pattern().alias.clone());

        // root match
        let root_match = MatchClause::Node(NodeMatch {
            var: root.id,
            labels: self.node_pattern().labels,
            id_filter: self.node_pattern().id.clone().map(Into::into), // typed id -> i64
            property_filters: self.node_pattern().property_filters.clone(),
        });

        let mut matches = vec![root_match];

        // traversals: minimal = treat each step as a hop from root for now
        // (later: chain them properly for multi-hop)
        let mut current = root.id;

        for step in &self.node_pattern().traversals {
            // allocate vars for rel and end node
            let rel_id = vg.fresh();
            let end_id = vg.fresh();

            // add hop clause
            matches.push(MatchClause::Hop(HopMatch {
                start: current,
                rel_type: step.rel_type,
                rel_var: rel_id,
                dir: step.dir,
                end: end_id,
                end_labels: step.end_labels,
            }));

            // ALWAYS add end-node match (compiler invariant)
            matches.push(MatchClause::Node(NodeMatch {
                var: end_id,
                labels: step.end_labels,
                id_filter: None,
                property_filters: step.end_filters.clone(), // may be empty
            }));

            current = end_id;
        }

        GraphQuery {
            matches,
            where_: vec![], // you can later promote some filters here
            ret: Return::Node(root.id),
            limit: self.limit_value(),
            offset: self.offset_value(),
        }
    }
}
