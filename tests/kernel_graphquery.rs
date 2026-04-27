mod common;
use crate::common::*;

use serde_json::json;
use std::collections::BTreeMap;

use grm_rs::dsl::{GraphQuery, KernelValue, MatchClause, NodeMatch, Return};
use grm_rs::{GraphBackend, GraphTx, InMemoryBackend, NodePattern, Query, Result, VarGen};

#[tokio::test]
async fn in_memory_backend_create_and_match_node_via_graphquery() {
    let backend = InMemoryBackend::new();
    let mut tx = backend.begin_tx().await.expect("begin tx failed");

    let mut props = BTreeMap::new();
    props.insert("name".to_string(), json!("Alice"));
    let node = tx
        .create_node(vec!["User".to_string()], props)
        .await
        .expect("create_node failed");
    let created_id = node.id;

    tx.commit().await.expect("commit failed");

    let mut vg = VarGen::default();
    let root = vg.fresh();

    let gq = GraphQuery {
        matches: vec![MatchClause::Node(NodeMatch {
            var: root,
            labels: &["User"],
            id_filter: Some(created_id),
            property_filters: vec![],
        })],
        where_: vec![],
        ret: Return::Node(root),
        limit: None,
        offset: None,
    };

    let qr = backend
        .execute_graph(&gq)
        .await
        .expect("execute_graph failed");
    assert_eq!(qr.rows.len(), 1);

    let node = qr.rows[0].get_returned(&gq).unwrap().as_node().unwrap();
    assert_eq!(node.id, created_id);
    assert_eq!(node.props["name"], "Alice");
}

#[tokio::test]
async fn execute_graph_out_any_matches_any_relationship_type() -> Result<()> {
    let backend = InMemoryBackend::new();

    let user_id: i64;
    {
        let mut tx = backend.begin_tx().await?;

        let u = tx
            .create_node(vec!["User".to_string()], Default::default())
            .await?;
        let p = tx
            .create_node(vec!["Post".to_string()], Default::default())
            .await?;

        tx.create_relationship(u.id, p.id, "LIKED", Default::default())
            .await?;

        tx.commit().await?;
        user_id = u.id;
    }

    let q = Query::<User>::matching(NodePattern::<User>::new().out_any().to::<Post>());
    let gq = q.compile_to_graph();

    let mut tx = backend.begin_tx().await?;
    let qr = tx.execute_graph(&gq).await?;
    tx.commit().await?;

    let got_ids: Vec<i64> = qr
        .rows
        .iter()
        .filter_map(|row| {
            row.values.values().next().and_then(|v| match v {
                KernelValue::Node(n) => Some(n.id),
                _ => panic!("expected node"),
            })
        })
        .collect();

    assert!(
        got_ids.contains(&user_id),
        "expected out_any traversal to match User via non-typed relationship"
    );

    Ok(())
}
