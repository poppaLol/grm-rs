mod common;

use std::collections::BTreeMap;
use serde_json::json;

use grm_rs::{GraphBackend, GraphTx, InMemoryBackend};

#[tokio::test]
async fn transaction_rollback_on_error() {
    let backend = InMemoryBackend::new();

    let mut tx = backend.begin_tx().await.expect("begin_tx failed");

    let mut props = BTreeMap::new();
    props.insert("name".to_string(), json!("TempUser"));

    let node = tx
        .create_node(vec!["User".to_string()], props)
        .await
        .expect("create_node in tx failed");

    let temp_id = node.id;

    let err = tx.execute_query("XXXX NOT SUPPORTED XXXX", json!({})).await;
    assert!(err.is_err(), "expected unsupported query to fail");

    tx.rollback().await.expect("rollback failed");

    let mut tx2 = backend.begin_tx().await.expect("begin_tx failed");
    let found = tx2
        .find_node_by_id(temp_id)
        .await
        .expect("find_node_by_id failed");
    tx2.commit().await.expect("commit failed");

    assert!(
        found.is_none(),
        "node created in rolled-back tx should not be visible"
    );
}

#[tokio::test]
async fn simple_transaction_rollback_discards_changes() {
    let backend = InMemoryBackend::new();

    let mut tx = backend.begin_tx().await.expect("begin_tx failed");

    let mut props = BTreeMap::new();
    props.insert("name".to_string(), json!("TempUser"));

    let node = tx
        .create_node(vec!["User".to_string()], props)
        .await
        .unwrap();
    let temp_id = node.id;

    tx.rollback().await.unwrap();

    let mut tx2 = backend.begin_tx().await.unwrap();
    let found = tx2.find_node_by_id(temp_id).await.unwrap();
    tx2.commit().await.unwrap();

    assert!(found.is_none());
}
