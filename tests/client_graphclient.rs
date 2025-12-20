mod common;
use crate::common::*;

use std::collections::BTreeMap;
use serde_json::json;

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

    let q = Query::<User>::matching(
        NodePattern::<User>::new().filter(User::name_prop().eq("Alice")),
    );

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
