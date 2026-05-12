use std::collections::{BTreeMap, BTreeSet};

use grm_rs::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use grm_rs::{
    BackendIdType, BackendIdentity, CompareOp, ExecutionPlan, GraphBackend, GraphTx, GrmError,
    InMemoryBackend, KernelValue, PlanStepKind, PropertyFilter, Result, ReturnKind,
};
use serde_json::json;

fn user_to_post_query(return_value: Return) -> GraphQuery {
    let root = VarId(0);
    let rel = VarId(1);
    let end = VarId(2);

    GraphQuery {
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
        ret: return_value,
        limit: None,
        offset: None,
    }
}

fn user_to_filtered_post_query() -> GraphQuery {
    let mut query = user_to_post_query(Return::Node(VarId(2)));
    query.matches.push(MatchClause::Node(NodeMatch {
        var: VarId(2),
        labels: &["Post"],
        id_filter: None,
        property_filters: vec![PropertyFilter {
            key: "title",
            op: CompareOp::Eq,
            value: json!("Hello"),
        }],
    }));
    query
}

async fn seed_user_post(
    tx: &mut (impl GraphTx + Send),
    user_name: &str,
    post_title: &str,
) -> Result<(i64, i64, i64)> {
    let user = tx
        .create_node(
            vec!["User".to_string()],
            BTreeMap::from([("name".to_string(), json!(user_name))]),
        )
        .await?;
    let post = tx
        .create_node(
            vec!["Post".to_string()],
            BTreeMap::from([("title".to_string(), json!(post_title))]),
        )
        .await?;
    let rel = tx
        .create_relationship(user.id, post.id, "Authored", BTreeMap::new())
        .await?;

    Ok((user.id, post.id, rel.id))
}

#[tokio::test]
async fn backend_capabilities_and_id_contract_are_explicit() {
    let backend = InMemoryBackend::new();
    let capabilities = backend.capabilities();

    assert!(capabilities.graph_query);
    assert!(!capabilities.string_query);
    assert!(capabilities.transactions);
    assert!(capabilities.read_your_writes);
    assert!(capabilities.rollback);
    assert_eq!(backend.node_id_type(), BackendIdType::Int64);
    assert_eq!(backend.rel_id_type(), BackendIdType::Int64);
}

#[tokio::test]
async fn query_result_rows_include_bound_vars_and_return_var_shape() -> Result<()> {
    let backend = InMemoryBackend::new();
    let mut tx = backend.begin_tx().await?;
    seed_user_post(&mut tx, "Alice", "Hello").await?;

    let query = user_to_post_query(Return::Node(VarId(2)));
    query.validate()?;

    let result = tx.execute_graph(&query).await?;

    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    let bound: BTreeSet<_> = query.bound_vars().into_iter().collect();
    let keys: BTreeSet<_> = row.keys().copied().collect();
    assert_eq!(keys, bound);
    assert!(matches!(
        row.get(&query.return_var()),
        Some(KernelValue::Node(node)) if node.props.get("title") == Some(&json!("Hello"))
    ));
    assert!(matches!(query.return_kind(), ReturnKind::Node));

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn traversal_row_binding_can_return_relationships() -> Result<()> {
    let backend = InMemoryBackend::new();
    let mut tx = backend.begin_tx().await?;
    let (_user_id, _post_id, rel_id) = seed_user_post(&mut tx, "Alice", "Hello").await?;

    let query = user_to_post_query(Return::Rel(VarId(1)));
    let result = tx.execute_graph(&query).await?;

    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert!(matches!(
        row.get(&query.return_var()),
        Some(KernelValue::Rel(rel)) if rel.id == rel_id && rel.ty == "Authored"
    ));
    assert!(matches!(query.return_kind(), ReturnKind::Rel));

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn transaction_read_your_writes_commit_visibility_and_rollback() -> Result<()> {
    let backend = InMemoryBackend::new();

    let committed_id = {
        let mut tx = backend.begin_tx().await?;
        let created = tx
            .create_node(
                vec!["User".to_string()],
                BTreeMap::from([("name".to_string(), json!("Committed"))]),
            )
            .await?;
        assert!(tx.find_node_by_id(created.id).await?.is_some());
        tx.commit().await?;
        created.id
    };

    {
        let mut read_tx = backend.begin_tx().await?;
        assert!(read_tx.find_node_by_id(committed_id).await?.is_some());
        read_tx.commit().await?;
    }

    let rolled_back_id = {
        let mut tx = backend.begin_tx().await?;
        let created = tx
            .create_node(
                vec!["User".to_string()],
                BTreeMap::from([("name".to_string(), json!("RolledBack"))]),
            )
            .await?;
        assert!(tx.find_node_by_id(created.id).await?.is_some());
        tx.rollback().await?;
        created.id
    };

    let mut read_tx = backend.begin_tx().await?;
    assert!(read_tx.find_node_by_id(rolled_back_id).await?.is_none());
    read_tx.commit().await?;

    Ok(())
}

#[tokio::test]
async fn delete_node_hides_node_and_incident_relationships() -> Result<()> {
    let backend = InMemoryBackend::new();
    let (user_id, post_id, rel_id) = {
        let mut tx = backend.begin_tx().await?;
        let ids = seed_user_post(&mut tx, "Alice", "Hello").await?;
        tx.commit().await?;
        ids
    };

    let mut tx = backend.begin_tx().await?;
    tx.delete_node(user_id).await?;

    assert!(tx.find_node_by_id(user_id).await?.is_none());
    assert!(tx.outgoing(user_id, Some("Authored")).await?.is_empty());
    assert!(tx.incoming(post_id, Some("Authored")).await?.is_empty());
    assert!(
        tx.update_relationship(rel_id, BTreeMap::new())
            .await?
            .is_none()
    );

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn unsupported_string_queries_return_a_practical_backend_error() -> Result<()> {
    let backend = InMemoryBackend::new();
    let mut tx = backend.begin_tx().await?;

    let err = tx.execute_query("MATCH (n) RETURN n", json!({})).await;
    assert!(
        matches!(err, Err(GrmError::Backend(message)) if message.contains("does not support string queries"))
    );

    tx.rollback().await?;
    Ok(())
}

#[test]
fn execution_plan_vocabulary_renders_stable_debug_text() {
    let query = user_to_post_query(Return::Node(VarId(2)));
    let plan = ExecutionPlan::for_graph_query(&query);

    assert!(matches!(
        plan.steps[0].kind,
        PlanStepKind::NodePropertySeek { var: VarId(0), ref key, .. } if key == "name"
    ));
    assert!(matches!(
        plan.steps[1].kind,
        PlanStepKind::ExpandOut {
            from: VarId(0),
            rel: VarId(1),
            to: VarId(2),
            ..
        }
    ));
    assert!(matches!(
        plan.steps[2].kind,
        PlanStepKind::Return {
            var: VarId(2),
            kind: ReturnKind::Node
        }
    ));

    assert_eq!(
        plan.to_string(),
        "1. NodePropertySeek v0 User.name\n2. ExpandOut v0 -[v1:Authored]-> v2\n3. Return Node v2"
    );
}

#[test]
fn execution_plan_renders_already_bound_node_clauses_as_filters() {
    let query = user_to_filtered_post_query();
    let plan = ExecutionPlan::for_graph_query(&query);

    assert!(matches!(
        plan.steps[2].kind,
        PlanStepKind::NodeFilter {
            var: VarId(2),
            ref keys,
            ..
        } if keys == &vec!["title".to_string()]
    ));

    assert_eq!(
        plan.to_string(),
        "1. NodePropertySeek v0 User.name\n2. ExpandOut v0 -[v1:Authored]-> v2\n3. NodeFilter v2 Post title\n4. Return Node v2"
    );
}
