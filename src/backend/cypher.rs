use std::collections::BTreeMap;

use serde_json::Value;

use crate::dsl::{CompareOp, Direction, GraphQuery, MatchClause, NodeMatch, PropertyFilter, VarId};
use crate::{GrmError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct CypherQuery {
    pub text: String,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Default)]
struct Translator {
    params: BTreeMap<String, Value>,
    next_param: usize,
}

impl Translator {
    fn push_param(&mut self, value: Value) -> String {
        let name = format!("p{}", self.next_param);
        self.next_param += 1;
        self.params.insert(name.clone(), value);
        name
    }
}

pub fn graph_query_to_cypher(q: &GraphQuery) -> Result<CypherQuery> {
    q.validate()?;

    let mut translator = Translator::default();
    let node_matches = collect_node_matches(q);
    let pattern = build_match_pattern(q, &node_matches)?;
    let predicates = build_predicates(q, &node_matches, &mut translator);
    let return_clause = format!(
        "RETURN {}",
        q.bound_vars()
            .into_iter()
            .map(var_name)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut text = format!("MATCH {pattern}");
    if !predicates.is_empty() {
        text.push_str(" WHERE ");
        text.push_str(&predicates.join(" AND "));
    }
    text.push(' ');
    text.push_str(&return_clause);

    if let Some(offset) = q.offset {
        text.push_str(&format!(" SKIP {offset}"));
    }
    if let Some(limit) = q.limit {
        text.push_str(&format!(" LIMIT {limit}"));
    }

    Ok(CypherQuery {
        text,
        params: translator.params,
    })
}

fn collect_node_matches(q: &GraphQuery) -> BTreeMap<VarId, &NodeMatch> {
    q.matches
        .iter()
        .filter_map(|clause| match clause {
            MatchClause::Node(node) => Some((node.var, node)),
            MatchClause::Hop(_) => None,
        })
        .collect()
}

fn build_match_pattern(
    q: &GraphQuery,
    node_matches: &BTreeMap<VarId, &NodeMatch>,
) -> Result<String> {
    let mut clauses = q.matches.iter();
    let Some(MatchClause::Node(root)) = clauses.next() else {
        return Err(GrmError::Mapping(
            "GraphQuery must start with NodeMatch".into(),
        ));
    };

    let mut pattern = node_pattern(root.var, root.labels);

    for clause in clauses {
        match clause {
            MatchClause::Node(_) => {}
            MatchClause::Hop(hop) => {
                let end = node_matches.get(&hop.end);
                let end_labels = end
                    .map(|node| node.labels)
                    .filter(|labels| !labels.is_empty())
                    .unwrap_or(hop.end_labels);
                pattern.push_str(&relationship_pattern(hop.dir, hop.rel_var, hop.rel_type));
                pattern.push_str(&node_pattern(hop.end, end_labels));
            }
        }
    }

    Ok(pattern)
}

fn build_predicates(
    q: &GraphQuery,
    node_matches: &BTreeMap<VarId, &NodeMatch>,
    translator: &mut Translator,
) -> Vec<String> {
    let mut predicates = Vec::new();

    for node in node_matches.values() {
        let name = var_name(node.var);
        if let Some(id) = node.id_filter {
            let param = translator.push_param(Value::from(id));
            predicates.push(format!("id({name}) = ${param}"));
        }
        predicates.extend(
            node.property_filters
                .iter()
                .map(|filter| property_predicate(&name, filter, translator)),
        );
    }

    let root = q.root_var();
    let root_name = var_name(root);
    predicates.extend(
        q.where_
            .iter()
            .map(|filter| property_predicate(&root_name, filter, translator)),
    );

    predicates
}

fn property_predicate(var: &str, filter: &PropertyFilter, translator: &mut Translator) -> String {
    let param = translator.push_param(filter.value.clone());
    let prop = format!("{}.{}", var, property_key(filter.key));
    match filter.op {
        CompareOp::Eq => format!("{prop} = ${param}"),
        CompareOp::Ne => format!("{prop} <> ${param}"),
        CompareOp::Gt => format!("{prop} > ${param}"),
        CompareOp::Ge => format!("{prop} >= ${param}"),
        CompareOp::Lt => format!("{prop} < ${param}"),
        CompareOp::Le => format!("{prop} <= ${param}"),
        CompareOp::Contains => format!("{prop} CONTAINS ${param}"),
    }
}

fn node_pattern(var: VarId, labels: &[&str]) -> String {
    let labels = labels
        .iter()
        .map(|label| format!(":{}", cypher_name(label)))
        .collect::<String>();
    format!("({}{})", var_name(var), labels)
}

fn relationship_pattern(dir: Direction, var: VarId, rel_type: Option<&str>) -> String {
    let rel_type = rel_type
        .map(|ty| format!(":{}", cypher_name(ty)))
        .unwrap_or_default();
    match dir {
        Direction::Out => format!("-[{}{}]->", var_name(var), rel_type),
        Direction::In => format!("<-[{}{}]-", var_name(var), rel_type),
        Direction::Both => format!("-[{}{}]-", var_name(var), rel_type),
    }
}

fn property_key(key: &str) -> String {
    cypher_name(key)
}

fn cypher_name(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn var_name(var: VarId) -> String {
    format!("v{}", var.0)
}
