use std::collections::BTreeMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use grm_rs::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use grm_rs::{CompareOp, PropertyFilter, graph_query_to_cypher};
use neo4rs::{Graph, Node, Query, query};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires a running Neo4j Bolt endpoint and NEO4J_* env vars"]
async fn translated_one_hop_query_executes_against_neo4j() {
    let uri = env::var("NEO4J_URI").unwrap_or_else(|_| "host.docker.internal:7687".to_string());
    let user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
    let password =
        env::var("NEO4J_PASSWORD").expect("set NEO4J_PASSWORD to run the Neo4j smoke test");

    let graph = Graph::new(&uri, &user, &password)
        .await
        .expect("connect to Neo4j");

    let smoke_id = unique_smoke_id();
    println!("neo4j smoke_id={smoke_id}");
    graph
        .run(
            query(
                "CREATE (u:User {name: $name, smoke_id: $smoke_id})\
                 -[:AUTHORED {smoke_id: $smoke_id}]->\
                 (p:Post {title: $title, smoke_id: $smoke_id})",
            )
            .param("name", "Alice")
            .param("title", "Cypher Smoke")
            .param("smoke_id", smoke_id.clone()),
        )
        .await
        .expect("seed smoke graph");

    let cypher = graph_query_to_cypher(&one_hop_query(&smoke_id)).expect("translate GraphQuery");
    let mut rows = graph
        .execute(apply_params(query(&cypher.text), &cypher.params))
        .await
        .expect("execute translated Cypher");

    let row = rows
        .next()
        .await
        .expect("read row")
        .expect("translated query should return a row");
    let post: Node = row.get("v2").expect("read returned post node");
    let title: String = post.get("title").expect("read returned post title");
    assert_eq!(title, "Cypher Smoke");
    assert!(rows.next().await.expect("read row").is_none());

    cleanup_smoke_graph(&graph, &smoke_id).await;
}

fn one_hop_query(smoke_id: &str) -> GraphQuery {
    let root = VarId(0);
    let rel = VarId(1);
    let end = VarId(2);

    GraphQuery {
        matches: vec![
            MatchClause::Node(NodeMatch {
                var: root,
                labels: &["User"],
                id_filter: None,
                property_filters: vec![
                    PropertyFilter {
                        key: "name",
                        op: CompareOp::Eq,
                        value: json!("Alice"),
                    },
                    PropertyFilter {
                        key: "smoke_id",
                        op: CompareOp::Eq,
                        value: json!(smoke_id),
                    },
                ],
            }),
            MatchClause::Hop(HopMatch {
                start: root,
                rel_type: Some("AUTHORED"),
                rel_var: rel,
                dir: Direction::Out,
                end,
                end_labels: &["Post"],
            }),
            MatchClause::Node(NodeMatch {
                var: end,
                labels: &["Post"],
                id_filter: None,
                property_filters: vec![PropertyFilter {
                    key: "title",
                    op: CompareOp::Eq,
                    value: json!("Cypher Smoke"),
                }],
            }),
        ],
        where_: vec![],
        ret: Return::Node(end),
        limit: Some(1),
        offset: None,
    }
}

fn apply_params(mut query: Query, params: &BTreeMap<String, Value>) -> Query {
    for (key, value) in params {
        query = match value {
            Value::Bool(value) => query.param(key, *value),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    query.param(key, value)
                } else if let Some(value) =
                    value.as_u64().and_then(|value| i64::try_from(value).ok())
                {
                    query.param(key, value)
                } else if let Some(value) = value.as_f64() {
                    query.param(key, value)
                } else {
                    panic!("unsupported numeric Cypher parameter {key}: {value}");
                }
            }
            Value::String(value) => query.param(key, value.clone()),
            Value::Null | Value::Array(_) | Value::Object(_) => {
                panic!("unsupported Cypher parameter {key}: {value}");
            }
        };
    }
    query
}

async fn cleanup_smoke_graph(graph: &Graph, smoke_id: &str) {
    graph
        .run(
            query("MATCH (n {smoke_id: $smoke_id}) DETACH DELETE n")
                .param("smoke_id", smoke_id.to_string()),
        )
        .await
        .expect("cleanup smoke graph");
}

fn unique_smoke_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("grm-rs-smoke-{nanos}")
}
