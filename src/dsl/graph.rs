use crate::{GrmError, NodeModel, Props, Query, RelModel, Result};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    pub rel_type: Option<&'static str>,
    pub rel_var: VarId,
    pub dir: Direction,
    pub end: VarId,
    pub end_labels: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub enum Return {
    Node(VarId),
    Rel(VarId),
    // future: Tuple(Vec<VarId>), Subgraph, Path, etc.
}

#[derive(Debug, Clone)]
pub struct GraphQuery {
    pub matches: Vec<MatchClause>,
    pub where_: Vec<crate::dsl::PropertyFilter>, // AND semantics for now
    pub ret: Return,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl GraphQuery {
    pub fn return_node(var: VarId) -> Return {
        Return::Node(var)
    }

    pub fn return_rel(var: VarId) -> Return {
        Return::Rel(var)
    }

    pub fn return_var(&self) -> VarId {
        match self.ret {
            Return::Node(v) | Return::Rel(v) => v,
        }
    }

    pub fn return_kind(&self) -> ReturnKind {
        match self.ret {
            Return::Node(_) => ReturnKind::Node,
            Return::Rel(_) => ReturnKind::Rel,
        }
    }

    /// Convenience for executors / repos
    pub fn return_is_rel(&self) -> bool {
        matches!(self.ret, Return::Rel(_))
    }

    /// The first bound node var in the match chain.
    pub fn root_var(&self) -> VarId {
        match self.matches.first() {
            Some(MatchClause::Node(n)) => n.var,
            _ => panic!("GraphQuery invariant violated: first match clause must be Node"),
        }
    }

    /// The last bound node var in the match chain.
    /// If there are no hops, this is the root var.
    pub fn end_var(&self) -> VarId {
        let mut end = self.root_var();
        for mc in &self.matches {
            if let MatchClause::Hop(h) = mc {
                end = h.end;
            }
        }
        end
    }

    /// All vars introduced by this query (node + rel vars), in match order.
    pub fn bound_vars(&self) -> Vec<VarId> {
        let mut out = Vec::new();

        for mc in &self.matches {
            match mc {
                MatchClause::Node(n) => out.push(n.var),
                MatchClause::Hop(h) => {
                    out.push(h.rel_var);
                    out.push(h.end);
                }
            }
        }

        // Defensive: prevent accidental duplicates if compiler ever reuses vars.
        out.dedup();
        out
    }

    pub fn validate(&self) -> Result<()> {
        if self.matches.is_empty() {
            return Err(GrmError::Mapping("GraphQuery has no match clauses".into()));
        }

        // first must be node
        let root = match &self.matches[0] {
            MatchClause::Node(n) => n.var,
            _ => {
                return Err(GrmError::Mapping(
                    "GraphQuery must start with NodeMatch".into(),
                ));
            }
        };

        // ensure hop start refers to an already-bound node var
        let mut bound_nodes = std::collections::BTreeSet::new();
        bound_nodes.insert(root);

        for mc in &self.matches[1..] {
            match mc {
                MatchClause::Node(n) => {
                    bound_nodes.insert(n.var);
                }
                MatchClause::Hop(h) => {
                    if !bound_nodes.contains(&h.start) {
                        return Err(GrmError::Mapping(format!(
                            "HopMatch.start {:?} not bound before hop",
                            h.start
                        )));
                    }
                    bound_nodes.insert(h.end);
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnKind {
    Node,
    Rel,
}

#[derive(Debug, Clone)]
pub struct NodeValue {
    pub id: i64,
    pub labels: Vec<String>,
    pub props: Props,
}

#[derive(Debug, Clone)]
pub struct RelValue {
    pub id: i64,
    pub ty: String,
    pub from: i64,
    pub to: i64,
    pub props: Props,
}

#[derive(Debug, Clone)]
pub enum Value {
    Node(NodeValue),
    Rel(RelValue),
    Scalar(serde_json::Value),
}

impl Value {
    #[inline]
    pub fn as_node(&self) -> Option<&NodeValue> {
        match self {
            Value::Node(n) => Some(n),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReturnMode {
    #[default]
    Root,
    End,
    Rel,
}

fn end_var_from_matches(matches: &[MatchClause], root: VarId) -> VarId {
    let mut last = root;
    for m in matches {
        if let MatchClause::Node(nm) = m {
            last = nm.var;
        }
    }
    last
}

impl<N: NodeModel> Query<N> {
    pub fn compile_to_graph(&self) -> GraphQuery {
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

        // traversals with multi-hop chaining enabled
        let mut current = root.id;
        let mut last_rel_var: Option<VarId> = None;
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
            last_rel_var = Some(rel_id);
            current = end_id;
        }

        let end_var = end_var_from_matches(&matches, root.id);

        let return_var = match self.return_mode {
            ReturnMode::Root => Return::Node(root.id),
            ReturnMode::End => Return::Node(end_var),
            ReturnMode::Rel => {
                let rel_var = last_rel_var.expect("return_rel used with no traversal");
                Return::Rel(rel_var)
            }
        };

        GraphQuery {
            matches,
            where_: vec![],
            ret: return_var,
            limit: self.limit_value(),
            offset: self.offset_value(),
        }
    }
}
