use std::fmt;

use serde::{Deserialize, Serialize};

use crate::dsl::{
    CompareOp, Direction, GraphQuery, MatchClause, PropertyFilter, ReturnKind, VarId,
};

/// Lightweight backend capability hints.
///
/// These flags are intentionally descriptive rather than prescriptive. They let
/// tests and future planning/explain code describe what a backend can do without
/// forcing a trait redesign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackendCapabilities {
    pub graph_query: bool,
    pub string_query: bool,
    pub transactions: bool,
    pub read_your_writes: bool,
    pub rollback: bool,
}

/// Minimal logical execution-plan vocabulary used by tests, logs, and future
/// explain/profile work.
///
/// `for_graph_query` renders the current `GraphQuery` clauses into stable,
/// human-readable steps. It is not a cost model, does not reorder work, and does
/// not imply the backend must execute by literally interpreting these steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub steps: Vec<PlanStep>,
}

impl ExecutionPlan {
    pub fn new(steps: Vec<PlanStep>) -> Self {
        Self { steps }
    }

    pub fn for_graph_query(query: &GraphQuery) -> Self {
        let mut steps = Vec::new();
        let mut bound_nodes = Vec::new();

        for clause in &query.matches {
            match clause {
                MatchClause::Node(node) => {
                    let labels = node.labels.iter().map(|label| label.to_string()).collect();
                    if bound_nodes.contains(&node.var) {
                        steps.push(PlanStep::new(node_check_or_filter_step(
                            node.var,
                            labels,
                            node.id_filter,
                            &node.property_filters,
                        )));
                        continue;
                    }

                    if let Some(id) = node.id_filter {
                        steps.push(PlanStep::new(PlanStepKind::NodeById {
                            var: node.var,
                            labels,
                            id,
                        }));
                    } else if let Some(filter) = first_equality_filter(&node.property_filters) {
                        steps.push(PlanStep::new(PlanStepKind::NodePropertySeek {
                            var: node.var,
                            labels,
                            key: filter.key.to_string(),
                        }));
                    } else {
                        steps.push(PlanStep::new(PlanStepKind::NodeLabelScan {
                            var: node.var,
                            labels,
                        }));
                    }
                    bound_nodes.push(node.var);
                }
                MatchClause::Hop(hop) => {
                    let rel_type = hop.rel_type.map(str::to_string);
                    let kind = match hop.dir {
                        Direction::Out => PlanStepKind::ExpandOut {
                            from: hop.start,
                            rel: hop.rel_var,
                            to: hop.end,
                            rel_type,
                        },
                        Direction::In => PlanStepKind::ExpandIn {
                            from: hop.start,
                            rel: hop.rel_var,
                            to: hop.end,
                            rel_type,
                        },
                        Direction::Both => PlanStepKind::ExpandBoth {
                            from: hop.start,
                            rel: hop.rel_var,
                            to: hop.end,
                            rel_type,
                        },
                    };
                    steps.push(PlanStep::new(kind));
                    if !bound_nodes.contains(&hop.end) {
                        bound_nodes.push(hop.end);
                    }
                }
            }
        }

        steps.push(PlanStep::new(PlanStepKind::Return {
            var: query.return_var(),
            kind: query.return_kind(),
        }));

        Self { steps }
    }
}

impl fmt::Display for ExecutionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, step) in self.steps.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            write!(f, "{}. {}", idx + 1, step)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    pub kind: PlanStepKind,
}

impl PlanStep {
    pub fn new(kind: PlanStepKind) -> Self {
        Self { kind }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexMetadata {
    pub name: &'static str,
    pub kind: IndexKind,
    pub entity: IndexEntity,
    pub fields: &'static [&'static str],
    pub durable: bool,
    pub derived: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexKind {
    System,
    UserDefined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexEntity {
    Node,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessPath {
    NodeIdLookup,
    NodeLabelIndex,
    NodePropertyIndex,
    RelationshipIdLookup,
    RelationshipTypeIndex,
    RelationshipEndpointAdjacency,
    OutgoingAdjacency,
    IncomingAdjacency,
    BidirectionalAdjacency,
    Scan,
}

impl AccessPath {
    pub fn index_name(self) -> Option<&'static str> {
        match self {
            Self::NodeIdLookup => Some("system.node.id"),
            Self::NodeLabelIndex => Some("system.node.label"),
            Self::NodePropertyIndex => Some("system.node.property"),
            Self::RelationshipIdLookup => Some("system.edge.id"),
            Self::RelationshipTypeIndex => Some("system.edge.type"),
            Self::RelationshipEndpointAdjacency => None,
            Self::OutgoingAdjacency => Some("system.edge.outgoing_adjacency"),
            Self::IncomingAdjacency => Some("system.edge.incoming_adjacency"),
            Self::BidirectionalAdjacency => None,
            Self::Scan => None,
        }
    }

    pub fn is_scan(self) -> bool {
        matches!(self, Self::Scan)
    }
}

pub fn system_index_catalog() -> Vec<IndexMetadata> {
    vec![
        IndexMetadata {
            name: "system.node.id",
            kind: IndexKind::System,
            entity: IndexEntity::Node,
            fields: &["id"],
            durable: false,
            derived: true,
        },
        IndexMetadata {
            name: "system.node.label",
            kind: IndexKind::System,
            entity: IndexEntity::Node,
            fields: &["label"],
            durable: false,
            derived: true,
        },
        IndexMetadata {
            name: "system.node.property",
            kind: IndexKind::System,
            entity: IndexEntity::Node,
            fields: &["label", "property", "value"],
            durable: false,
            derived: true,
        },
        IndexMetadata {
            name: "system.edge.id",
            kind: IndexKind::System,
            entity: IndexEntity::Edge,
            fields: &["id"],
            durable: false,
            derived: true,
        },
        IndexMetadata {
            name: "system.edge.type",
            kind: IndexKind::System,
            entity: IndexEntity::Edge,
            fields: &["type"],
            durable: false,
            derived: true,
        },
        IndexMetadata {
            name: "system.edge.outgoing_adjacency",
            kind: IndexKind::System,
            entity: IndexEntity::Edge,
            fields: &["from", "type"],
            durable: false,
            derived: true,
        },
        IndexMetadata {
            name: "system.edge.incoming_adjacency",
            kind: IndexKind::System,
            entity: IndexEntity::Edge,
            fields: &["to", "type"],
            durable: false,
            derived: true,
        },
    ]
}

impl fmt::Display for PlanStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanStepKind {
    NodeById {
        var: VarId,
        labels: Vec<String>,
        id: i64,
    },
    NodeLabelScan {
        var: VarId,
        labels: Vec<String>,
    },
    NodePropertySeek {
        var: VarId,
        labels: Vec<String>,
        key: String,
    },
    NodeCheck {
        var: VarId,
        labels: Vec<String>,
    },
    NodeFilter {
        var: VarId,
        labels: Vec<String>,
        id: Option<i64>,
        keys: Vec<String>,
    },
    RelationshipById {
        var: VarId,
        rel_type: String,
        id: i64,
    },
    RelationshipTypeScan {
        var: VarId,
        rel_type: String,
    },
    RelationshipEndpointSeek {
        var: VarId,
        rel_type: String,
        from: Option<i64>,
        to: Option<i64>,
    },
    RelationshipFilter {
        var: VarId,
        rel_type: String,
        keys: Vec<String>,
    },
    ExpandOut {
        from: VarId,
        rel: VarId,
        to: VarId,
        rel_type: Option<String>,
    },
    ExpandIn {
        from: VarId,
        rel: VarId,
        to: VarId,
        rel_type: Option<String>,
    },
    ExpandBoth {
        from: VarId,
        rel: VarId,
        to: VarId,
        rel_type: Option<String>,
    },
    Return {
        var: VarId,
        kind: ReturnKind,
    },
}

impl PlanStepKind {
    pub fn logical_name(&self) -> &'static str {
        match self {
            Self::NodeById { .. } => "NodeById",
            Self::NodeLabelScan { .. } => "NodeLabelScan",
            Self::NodePropertySeek { .. } => "NodePropertySeek",
            Self::NodeCheck { .. } => "NodeCheck",
            Self::NodeFilter { .. } => "NodeFilter",
            Self::RelationshipById { .. } => "RelationshipById",
            Self::RelationshipTypeScan { .. } => "RelationshipTypeScan",
            Self::RelationshipEndpointSeek { .. } => "RelationshipEndpointSeek",
            Self::RelationshipFilter { .. } => "RelationshipFilter",
            Self::ExpandOut { .. } => "ExpandOut",
            Self::ExpandIn { .. } => "ExpandIn",
            Self::ExpandBoth { .. } => "ExpandBoth",
            Self::Return { .. } => "Return",
        }
    }

    pub fn access_path(&self) -> Option<AccessPath> {
        match self {
            Self::NodeById { .. } => Some(AccessPath::NodeIdLookup),
            Self::NodeLabelScan { labels, .. } => {
                if labels.is_empty() {
                    Some(AccessPath::Scan)
                } else {
                    Some(AccessPath::NodeLabelIndex)
                }
            }
            Self::NodePropertySeek { .. } => Some(AccessPath::NodePropertyIndex),
            Self::RelationshipById { .. } => Some(AccessPath::RelationshipIdLookup),
            Self::RelationshipTypeScan { .. } => Some(AccessPath::RelationshipTypeIndex),
            Self::RelationshipEndpointSeek { from, to, .. } => match (from, to) {
                (Some(_), Some(_)) => Some(AccessPath::RelationshipEndpointAdjacency),
                (Some(_), None) => Some(AccessPath::OutgoingAdjacency),
                (None, Some(_)) => Some(AccessPath::IncomingAdjacency),
                (None, None) => Some(AccessPath::RelationshipTypeIndex),
            },
            Self::ExpandOut { .. } => Some(AccessPath::OutgoingAdjacency),
            Self::ExpandIn { .. } => Some(AccessPath::IncomingAdjacency),
            Self::ExpandBoth { .. } => Some(AccessPath::BidirectionalAdjacency),
            Self::NodeFilter { .. } | Self::RelationshipFilter { .. } => Some(AccessPath::Scan),
            Self::NodeCheck { .. } | Self::Return { .. } => None,
        }
    }

    pub fn candidate_index_names(&self) -> Vec<&'static str> {
        match self {
            Self::RelationshipEndpointSeek {
                from: Some(_),
                to: Some(_),
                ..
            } => vec![
                "system.edge.outgoing_adjacency",
                "system.edge.incoming_adjacency",
            ],
            Self::ExpandBoth { .. } => vec![
                "system.edge.outgoing_adjacency",
                "system.edge.incoming_adjacency",
            ],
            _ => self
                .access_path()
                .and_then(|path| path.index_name())
                .into_iter()
                .collect(),
        }
    }
}

impl fmt::Display for PlanStepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeById { var, labels, id } => {
                write!(
                    f,
                    "NodeById {} {} id={}",
                    fmt_var(*var),
                    fmt_labels(labels),
                    id
                )
            }
            Self::NodeLabelScan { var, labels } => {
                write!(f, "NodeLabelScan {} {}", fmt_var(*var), fmt_labels(labels))
            }
            Self::NodePropertySeek { var, labels, key } => {
                write!(
                    f,
                    "NodePropertySeek {} {}.{}",
                    fmt_var(*var),
                    fmt_labels(labels),
                    key
                )
            }
            Self::NodeCheck { var, labels } => {
                write!(f, "NodeCheck {} {}", fmt_var(*var), fmt_labels(labels))
            }
            Self::NodeFilter {
                var,
                labels,
                id,
                keys,
            } => {
                write!(f, "NodeFilter {} {}", fmt_var(*var), fmt_labels(labels))?;
                if let Some(id) = id {
                    write!(f, " id={id}")?;
                }
                if !keys.is_empty() {
                    write!(f, " {}", keys.join(","))?;
                }
                Ok(())
            }
            Self::RelationshipById { var, rel_type, id } => {
                write!(
                    f,
                    "RelationshipById {} :{} id={}",
                    fmt_var(*var),
                    rel_type,
                    id
                )
            }
            Self::RelationshipTypeScan { var, rel_type } => {
                write!(f, "RelationshipTypeScan {} :{}", fmt_var(*var), rel_type)
            }
            Self::RelationshipEndpointSeek {
                var,
                rel_type,
                from,
                to,
            } => {
                write!(
                    f,
                    "RelationshipEndpointSeek {} :{}",
                    fmt_var(*var),
                    rel_type
                )?;
                if let Some(from) = from {
                    write!(f, " from={from}")?;
                }
                if let Some(to) = to {
                    write!(f, " to={to}")?;
                }
                Ok(())
            }
            Self::RelationshipFilter {
                var,
                rel_type,
                keys,
            } => {
                write!(f, "RelationshipFilter {} :{}", fmt_var(*var), rel_type)?;
                if !keys.is_empty() {
                    write!(f, " {}", keys.join(","))?;
                }
                Ok(())
            }
            Self::ExpandOut {
                from,
                rel,
                to,
                rel_type,
            } => write!(
                f,
                "ExpandOut {} -[{}{}]-> {}",
                fmt_var(*from),
                fmt_var(*rel),
                fmt_rel_type(rel_type),
                fmt_var(*to)
            ),
            Self::ExpandIn {
                from,
                rel,
                to,
                rel_type,
            } => write!(
                f,
                "ExpandIn {} <-[{}{}]- {}",
                fmt_var(*from),
                fmt_var(*rel),
                fmt_rel_type(rel_type),
                fmt_var(*to)
            ),
            Self::ExpandBoth {
                from,
                rel,
                to,
                rel_type,
            } => write!(
                f,
                "ExpandBoth {} -[{}{}]- {}",
                fmt_var(*from),
                fmt_var(*rel),
                fmt_rel_type(rel_type),
                fmt_var(*to)
            ),
            Self::Return { var, kind } => write!(f, "Return {:?} {}", kind, fmt_var(*var)),
        }
    }
}

fn node_check_or_filter_step(
    var: VarId,
    labels: Vec<String>,
    id: Option<i64>,
    filters: &[PropertyFilter],
) -> PlanStepKind {
    let keys = filters
        .iter()
        .map(|filter| filter.key.to_string())
        .collect::<Vec<_>>();

    if id.is_some() || !keys.is_empty() {
        PlanStepKind::NodeFilter {
            var,
            labels,
            id,
            keys,
        }
    } else {
        PlanStepKind::NodeCheck { var, labels }
    }
}

fn first_equality_filter(filters: &[PropertyFilter]) -> Option<&PropertyFilter> {
    filters.iter().find(|filter| filter.op == CompareOp::Eq)
}

fn fmt_var(var: VarId) -> String {
    format!("v{}", var.0)
}

fn fmt_labels(labels: &[String]) -> String {
    if labels.is_empty() {
        "*".to_string()
    } else {
        labels.join("+")
    }
}

fn fmt_rel_type(rel_type: &Option<String>) -> String {
    rel_type
        .as_ref()
        .map(|ty| format!(":{ty}"))
        .unwrap_or_default()
}
