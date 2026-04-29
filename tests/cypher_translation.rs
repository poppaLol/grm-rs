mod common;

use common::{Post, User};
use grm_rs::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use grm_rs::{CompareOp, NodeModel, PropertyFilter, graph_query_to_cypher};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn translates_root_node_match_with_filters_and_paging() {
    let root = VarId(0);
    let query = GraphQuery {
        matches: vec![MatchClause::Node(NodeMatch {
            var: root,
            labels: &["User"],
            id_filter: Some(42),
            property_filters: vec![
                PropertyFilter {
                    key: "name",
                    op: CompareOp::Eq,
                    value: json!("Alice"),
                },
                PropertyFilter {
                    key: "age",
                    op: CompareOp::Ge,
                    value: json!(21),
                },
            ],
        })],
        where_: vec![],
        ret: Return::Node(root),
        limit: Some(10),
        offset: Some(5),
    };

    let cypher = graph_query_to_cypher(&query).expect("translation should succeed");

    assert_eq!(
        cypher.text,
        "MATCH (v0:`User`) WHERE id(v0) = $p0 AND v0.`name` = $p1 AND v0.`age` >= $p2 RETURN v0 SKIP 5 LIMIT 10"
    );
    assert_eq!(
        cypher.params,
        params([("p0", json!(42)), ("p1", json!("Alice")), ("p2", json!(21))])
    );
}

#[test]
fn translates_one_hop_outgoing_traversal_returning_end_node() {
    let root = VarId(0);
    let rel = VarId(1);
    let end = VarId(2);
    let query = GraphQuery {
        matches: vec![
            MatchClause::Node(NodeMatch {
                var: root,
                labels: User::LABELS,
                id_filter: None,
                property_filters: vec![PropertyFilter {
                    key: "name",
                    op: CompareOp::Contains,
                    value: json!("Ali"),
                }],
            }),
            MatchClause::Hop(HopMatch {
                start: root,
                rel_type: Some("AUTHORED"),
                rel_var: rel,
                dir: Direction::Out,
                end,
                end_labels: Post::LABELS,
            }),
            MatchClause::Node(NodeMatch {
                var: end,
                labels: Post::LABELS,
                id_filter: None,
                property_filters: vec![PropertyFilter {
                    key: "title",
                    op: CompareOp::Ne,
                    value: json!("Draft"),
                }],
            }),
        ],
        where_: vec![],
        ret: Return::Node(end),
        limit: None,
        offset: None,
    };

    let cypher = graph_query_to_cypher(&query).expect("translation should succeed");

    assert_eq!(
        cypher.text,
        "MATCH (v0:`User`)-[v1:`AUTHORED`]->(v2:`Post`) WHERE v0.`name` CONTAINS $p0 AND v2.`title` <> $p1 RETURN v2"
    );
    assert_eq!(
        cypher.params,
        params([("p0", json!("Ali")), ("p1", json!("Draft"))])
    );
}

#[test]
fn translates_incoming_any_relationship_returning_relationship() {
    let root = VarId(0);
    let rel = VarId(1);
    let end = VarId(2);
    let query = GraphQuery {
        matches: vec![
            MatchClause::Node(NodeMatch {
                var: root,
                labels: &["Post"],
                id_filter: None,
                property_filters: vec![],
            }),
            MatchClause::Hop(HopMatch {
                start: root,
                rel_type: None,
                rel_var: rel,
                dir: Direction::In,
                end,
                end_labels: &["User"],
            }),
            MatchClause::Node(NodeMatch {
                var: end,
                labels: &["User"],
                id_filter: None,
                property_filters: vec![],
            }),
        ],
        where_: vec![],
        ret: Return::Rel(rel),
        limit: Some(3),
        offset: None,
    };

    let cypher = graph_query_to_cypher(&query).expect("translation should succeed");

    assert_eq!(
        cypher.text,
        "MATCH (v0:`Post`)<-[v1]-(v2:`User`) RETURN v1 LIMIT 3"
    );
    assert_eq!(cypher.params, params([]));
}

#[test]
fn escapes_cypher_names_with_backticks() {
    let root = VarId(0);
    let query = GraphQuery {
        matches: vec![MatchClause::Node(NodeMatch {
            var: root,
            labels: &["Odd`Label"],
            id_filter: None,
            property_filters: vec![PropertyFilter {
                key: "odd`key",
                op: CompareOp::Eq,
                value: json!(true),
            }],
        })],
        where_: vec![],
        ret: Return::Node(root),
        limit: None,
        offset: None,
    };

    let cypher = graph_query_to_cypher(&query).expect("translation should succeed");

    assert_eq!(
        cypher.text,
        "MATCH (v0:`Odd``Label`) WHERE v0.`odd``key` = $p0 RETURN v0"
    );
    assert_eq!(cypher.params, params([("p0", json!(true))]));
}

fn params<const N: usize>(
    entries: [(&str, serde_json::Value); N],
) -> BTreeMap<String, serde_json::Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}
