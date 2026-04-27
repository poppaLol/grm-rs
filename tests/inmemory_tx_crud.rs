mod common;

use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

use grm_rs::{GraphBackend, GraphTx, InMemoryBackend, Result};

#[tokio::test]
async fn in_memory_backend_create_and_match_node() {
    let backend = InMemoryBackend::new();
    let mut tx = backend.begin_tx().await.expect("begin tx failed");

    let mut props = BTreeMap::new();
    props.insert("name".to_string(), json!("Alice"));

    let node = tx
        .create_node(vec!["User".to_string()], props)
        .await
        .expect("create_node failed");

    let created_id = node.id;
    assert_eq!(node.props.get("name").unwrap(), &json!("Alice"));

    let found = tx
        .find_node_by_id(created_id)
        .await
        .expect("find_node_by_id failed")
        .expect("node not found");

    assert_eq!(found.id, created_id);
    assert_eq!(found.props.get("name").unwrap(), &json!("Alice"));

    tx.commit().await.expect("commit failed");
}

#[tokio::test]
async fn tx_incoming_returns_from_node_for_matching_type() -> Result<()> {
    let backend = InMemoryBackend::new();

    let (a_id, b_id, rel_type) = {
        let mut tx = backend.begin_tx().await?;
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;
        let rel_type = "R";

        tx.create_relationship(a.id, b.id, rel_type, Default::default())
            .await?;
        tx.commit().await?;
        (a.id, b.id, rel_type)
    };

    let mut tx = backend.begin_tx().await?;

    let incoming_to_b = tx.incoming(b_id, Some(&rel_type)).await?;
    assert_eq!(incoming_to_b.len(), 1);

    let (_rel, from_node) = &incoming_to_b[0];
    assert_eq!(from_node.id, a_id);

    let incoming_to_a = tx.incoming(a_id, Some(&rel_type)).await?;
    assert!(incoming_to_a.is_empty());

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn tx_both_returns_neighbors_from_outgoing_and_incoming() -> Result<()> {
    let backend = InMemoryBackend::new();

    let (a_id, b_id, c_id, rel_type) = {
        let mut tx = backend.begin_tx().await?;

        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;
        let c = tx
            .create_node(vec!["C".to_string()], Default::default())
            .await?;

        let rel_type = "R";
        tx.create_relationship(c.id, a.id, rel_type, Default::default())
            .await?;
        tx.create_relationship(a.id, b.id, rel_type, Default::default())
            .await?;

        tx.commit().await?;
        (a.id, b.id, c.id, rel_type)
    };

    let mut tx = backend.begin_tx().await?;

    let pairs = tx.both(a_id, Some(&rel_type)).await?;

    let neighbor_ids: BTreeSet<i64> = pairs.into_iter().map(|(_rel, n)| n.id).collect();
    let expected: BTreeSet<i64> = [b_id, c_id].into_iter().collect();

    assert_eq!(neighbor_ids, expected);

    tx.commit().await?;
    Ok(())
}
