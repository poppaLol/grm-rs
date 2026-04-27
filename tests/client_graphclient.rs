mod common;
use crate::common::*;

use serde_json::json;
use std::collections::BTreeMap;

use grm_rs::{GraphClient, GraphTx, InMemoryBackend, NodePattern, Query};

#[tokio::test]
async fn in_memory_backend_create_and_find_by_name_with_client() {
    let backend = InMemoryBackend::new();
    let client = GraphClient::new(backend);

    let mut tx = client.transaction().await.expect("begin tx failed");

    let mut props = BTreeMap::new();
    props.insert("name".to_string(), json!("Alice"));

    tx.tx_mut()
        .expect("tx already finished")
        .create_node(vec!["User".to_string()], props)
        .await
        .expect("create_node failed");

    let q =
        Query::<User>::matching(NodePattern::<User>::new().filter(User::name_prop().eq("Alice")));

    let exec = tx.execute(q).await.expect("execute_graph failed");
    assert_eq!(exec.qr.rows.len(), 1);

    let node = exec.qr.rows[0]
        .get_returned(&exec.gq)
        .unwrap()
        .as_node()
        .unwrap();

    assert_eq!(node.props["name"], "Alice");

    tx.commit().await.expect("commit failed");
}

#[tokio::test]
async fn tx_query_rel_returns_typed_authored() {
    let backend = InMemoryBackend::new();
    let client = GraphClient::new(backend);

    let mut tx = client.transaction().await.expect("begin tx failed");

    // 1) Create (User) and (Post)
    let user = tx
        .tx_mut()
        .expect("tx finished")
        .create_node(vec!["User".to_string()], Default::default())
        .await
        .expect("create user failed");

    let post = tx
        .tx_mut()
        .expect("tx finished")
        .create_node(vec!["Post".to_string()], Default::default())
        .await
        .expect("create post failed");

    // 2) Create AUTHORED relationship with props { year: 2024 }
    let mut rel_props = BTreeMap::new();
    rel_props.insert("year".to_string(), json!(2024));

    tx.tx_mut()
        .expect("tx finished")
        .create_relationship(user.id, post.id, "AUTHORED", rel_props)
        .await
        .expect("create relationship failed");

    // 3) Query: (User)-[:AUTHORED]->(Post) and return the relationship
    let q = Query::<User>::matching(NodePattern::<User>::new().out::<Authored>().to::<Post>())
        .return_rel();

    let rels: Vec<Authored> = tx.query_rel(q).await.expect("query_rel failed");
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].year, 2024);

    tx.commit().await.expect("commit failed");
}
