mod common;

use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

use grm_rs::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use grm_rs::{CompareOp, PropertyFilter};
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
async fn independent_write_transactions_merge_on_commit() -> Result<()> {
    let backend = InMemoryBackend::new();

    let mut tx1 = backend.begin_tx().await?;
    let alice = tx1
        .create_node(
            vec!["User".to_string()],
            BTreeMap::from([("name".to_string(), json!("Alice"))]),
        )
        .await?;

    let mut tx2 = backend.begin_tx().await?;
    let bob = tx2
        .create_node(
            vec!["User".to_string()],
            BTreeMap::from([("name".to_string(), json!("Bob"))]),
        )
        .await?;

    tx1.commit().await?;
    tx2.commit().await?;

    let mut read_tx = backend.begin_tx().await?;
    assert_eq!(
        read_tx
            .find_node_by_id(alice.id)
            .await?
            .unwrap()
            .props
            .get("name"),
        Some(&json!("Alice"))
    );
    assert_eq!(
        read_tx
            .find_node_by_id(bob.id)
            .await?
            .unwrap()
            .props
            .get("name"),
        Some(&json!("Bob"))
    );
    read_tx.commit().await?;

    Ok(())
}

#[tokio::test]
async fn materialized_read_transaction_does_not_replace_later_commits() -> Result<()> {
    let backend = InMemoryBackend::new();

    let mut read_tx = backend.begin_tx().await?;
    let initial = read_tx
        .find_nodes_by_property("name", &json!("Bob"))
        .await?;
    assert!(initial.is_empty());

    let mut write_tx = backend.begin_tx().await?;
    let bob = write_tx
        .create_node(
            vec!["User".to_string()],
            BTreeMap::from([("name".to_string(), json!("Bob"))]),
        )
        .await?;
    write_tx.commit().await?;

    read_tx.commit().await?;

    let mut verify_tx = backend.begin_tx().await?;
    assert!(verify_tx.find_node_by_id(bob.id).await?.is_some());
    verify_tx.commit().await?;

    Ok(())
}

#[tokio::test]
async fn property_lookup_read_does_not_materialize_working_copy() -> Result<()> {
    let backend = InMemoryBackend::new();

    {
        let mut tx = backend.begin_tx().await?;
        tx.create_node(
            vec!["User".to_string()],
            BTreeMap::from([("name".to_string(), json!("Alice"))]),
        )
        .await?;
        tx.commit().await?;
    }

    let mut tx = backend.begin_tx().await?;
    let users = tx.find_nodes_by_property("name", &json!("Alice")).await?;

    assert_eq!(users.len(), 1);
    assert!(
        tx.working_copy.is_none(),
        "property lookup should use overlay read-view"
    );

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn traversal_read_does_not_materialize_working_copy() -> Result<()> {
    let backend = InMemoryBackend::new();
    let user_id = {
        let mut tx = backend.begin_tx().await?;
        let user = tx
            .create_node(vec!["User".to_string()], Default::default())
            .await?;
        let post = tx
            .create_node(vec!["Post".to_string()], Default::default())
            .await?;
        tx.create_relationship(user.id, post.id, "Authored", Default::default())
            .await?;
        tx.commit().await?;
        user.id
    };

    let mut tx = backend.begin_tx().await?;
    let posts = tx.outgoing(user_id, Some("Authored")).await?;

    assert_eq!(posts.len(), 1);
    assert!(
        tx.working_copy.is_none(),
        "one-hop traversal should use overlay read-view"
    );

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn graph_query_read_does_not_materialize_working_copy() -> Result<()> {
    let backend = InMemoryBackend::new();
    {
        let mut tx = backend.begin_tx().await?;
        let user = tx
            .create_node(
                vec!["User".to_string()],
                BTreeMap::from([("name".to_string(), json!("Alice"))]),
            )
            .await?;
        let post = tx
            .create_node(vec!["Post".to_string()], Default::default())
            .await?;
        tx.create_relationship(user.id, post.id, "Authored", Default::default())
            .await?;
        tx.commit().await?;
    }

    let root = VarId(0);
    let rel = VarId(1);
    let end = VarId(2);
    let query = GraphQuery {
        matches: vec![
            MatchClause::Node(NodeMatch {
                var: root,
                labels: &["User"],
                id_filter: None,
                property_filters: vec![PropertyFilter {
                    key: "name",
                    op: CompareOp::Eq,
                    value: json!("Alice"),
                }],
            }),
            MatchClause::Hop(HopMatch {
                start: root,
                rel_type: Some("Authored"),
                rel_var: rel,
                dir: Direction::Out,
                end,
                end_labels: &["Post"],
            }),
        ],
        where_: vec![],
        ret: Return::Node(end),
        limit: None,
        offset: None,
    };

    let mut tx = backend.begin_tx().await?;
    let rows = tx.execute_graph(&query).await?;

    assert_eq!(rows.rows.len(), 1);
    assert!(
        tx.working_copy.is_none(),
        "graph query execution should use overlay read-view"
    );

    tx.commit().await?;
    Ok(())
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

    let incoming_to_b = tx.incoming(b_id, Some(rel_type)).await?;
    assert_eq!(incoming_to_b.len(), 1);

    let (_rel, from_node) = &incoming_to_b[0];
    assert_eq!(from_node.id, a_id);

    let incoming_to_a = tx.incoming(a_id, Some(rel_type)).await?;
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

    let pairs = tx.both(a_id, Some(rel_type)).await?;

    let neighbor_ids: BTreeSet<i64> = pairs.into_iter().map(|(_rel, n)| n.id).collect();
    let expected: BTreeSet<i64> = [b_id, c_id].into_iter().collect();

    assert_eq!(neighbor_ids, expected);

    tx.commit().await?;
    Ok(())
}
