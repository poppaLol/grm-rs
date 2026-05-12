use std::fmt;

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
