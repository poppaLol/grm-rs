use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use grm_rs::dsl::{Direction, HopMatch, MatchClause, NodeMatch, Return, VarId};
use grm_rs::{
    BackendIdType, BackendIdentity, CompareOp, GraphBackend, GraphQuery, GraphTx, GrmError,
    KernelValue, PropertyFilter, Result, ReturnKind,
};
use serde_json::{Value, json};

pub const BEHAVIOR_RUN_ID_PROP: &str = "grm_behavior_run_id";

#[derive(Debug, Clone, Copy)]
pub enum NativeQueryExpectation {
    Unsupported,
    Supported,
}

#[derive(Debug, Clone)]
pub struct BackendBehaviorConfig {
    pub run_id: String,
    pub native_query: NativeQueryExpectation,
}

impl BackendBehaviorConfig {
    pub fn in_memory() -> Self {
        Self {
            run_id: unique_smoke_id(),
            native_query: NativeQueryExpectation::Unsupported,
        }
    }
}

pub async fn run_shared_backend_behavior<B>(backend: B) -> Result<()>
where
    B: BackendIdentity + Clone,
{
    run_shared_backend_behavior_with_config(backend, BackendBehaviorConfig::in_memory()).await
}

pub async fn run_shared_backend_behavior_with_config<B>(
    backend: B,
    config: BackendBehaviorConfig,
) -> Result<()>
where
    B: BackendIdentity + Clone,
{
    capabilities_and_id_type_reporting(&backend, &config).await?;
    create_find_update_and_traverse(backend.clone(), &config).await?;
    delete_visibility_and_incident_relationships(backend.clone(), &config).await?;
    transaction_visibility(backend.clone(), &config).await?;
    rollback_discards_writes_updates_and_deletes(backend.clone(), &config).await?;
    execute_graph_row_shape(backend.clone(), &config).await?;
    traversal_query_returning_node_and_relationship(backend.clone(), &config).await?;
    native_query_behavior(backend, &config).await?;
    Ok(())
}

async fn capabilities_and_id_type_reporting<B>(
    backend: &B,
    config: &BackendBehaviorConfig,
) -> Result<()>
where
    B: BackendIdentity + Clone,
{
    let capabilities = backend.capabilities();
    assert!(capabilities.graph_query);
    assert!(capabilities.transactions);
    assert!(capabilities.read_your_writes);
    assert!(capabilities.rollback);
    assert_eq!(
        capabilities.string_query,
        matches!(config.native_query, NativeQueryExpectation::Supported)
    );
    assert_eq!(backend.node_id_type(), BackendIdType::Int64);
    assert_eq!(backend.rel_id_type(), BackendIdType::Int64);
    Ok(())
}

async fn create_find_update_and_traverse<B>(
    backend: B,
    config: &BackendBehaviorConfig,
) -> Result<()>
where
    B: GraphBackend + Clone,
{
    let smoke_id = unique_smoke_id();
    let mut tx = backend.begin_tx().await?;
    let user = tx
        .create_node(
            vec!["GrmBehaviorUser".to_string()],
            props_with_run(
                config,
                [
                    ("name", json!("Alice")),
                    ("smoke_id", json!(smoke_id.clone())),
                ],
            ),
        )
        .await?;
    let post = tx
        .create_node(
            vec!["GrmBehaviorPost".to_string()],
            props_with_run(
                config,
                [
                    ("title", json!("Shared Backend Behavior")),
                    ("smoke_id", json!(smoke_id.clone())),
                ],
            ),
        )
        .await?;
    let rel = tx
        .create_relationship(
            user.id,
            post.id,
            "GRM_BEHAVIOR_AUTHORED",
            props_with_run(config, [("smoke_id", json!(smoke_id.clone()))]),
        )
        .await?;

    let found_by_id = tx.find_node_by_id(user.id).await?.expect("node by id");
    assert_eq!(found_by_id.id, user.id);
    assert_eq!(found_by_id.props.get("name"), Some(&json!("Alice")));

    let found_by_property = tx
        .find_nodes_by_property("smoke_id", &json!(smoke_id))
        .await?;
    assert_eq!(
        ids(found_by_property.iter().map(|node| node.id)),
        ids([user.id, post.id])
    );

    let updated_user = tx
        .update_node(user.id, props([("name", json!("Alicia"))]))
        .await?
        .expect("updated node");
    assert_eq!(updated_user.props.get("name"), Some(&json!("Alicia")));

    let updated_rel = tx
        .update_relationship(rel.id, props([("weight", json!(2))]))
        .await?
        .expect("updated relationship");
    assert_eq!(updated_rel.props.get("weight"), Some(&json!(2)));

    let outgoing = tx.outgoing(user.id, Some("GRM_BEHAVIOR_AUTHORED")).await?;
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].0.id, rel.id);
    assert_eq!(outgoing[0].1.id, post.id);

    let incoming = tx.incoming(post.id, Some("GRM_BEHAVIOR_AUTHORED")).await?;
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].0.id, rel.id);
    assert_eq!(incoming[0].1.id, user.id);

    let both_from_user = tx.both(user.id, Some("GRM_BEHAVIOR_AUTHORED")).await?;
    assert_eq!(both_from_user.len(), 1);
    assert_eq!(both_from_user[0].1.id, post.id);

    let all_outgoing = tx.outgoing(user.id, None).await?;
    assert!(
        all_outgoing
            .iter()
            .any(|(candidate, _)| candidate.id == rel.id)
    );

    tx.commit().await
}

async fn delete_visibility_and_incident_relationships<B>(
    backend: B,
    config: &BackendBehaviorConfig,
) -> Result<()>
where
    B: GraphBackend + Clone,
{
    let mut tx = backend.begin_tx().await?;
    let left = tx
        .create_node(
            vec!["GrmBehaviorDeleteLeft".to_string()],
            props_with_run(config, []),
        )
        .await?;
    let right = tx
        .create_node(
            vec!["GrmBehaviorDeleteRight".to_string()],
            props_with_run(config, []),
        )
        .await?;
    let rel = tx
        .create_relationship(
            left.id,
            right.id,
            "GRM_BEHAVIOR_DELETE",
            props_with_run(config, []),
        )
        .await?;
    tx.commit().await?;

    let mut tx = backend.begin_tx().await?;
    tx.delete_relationship(rel.id).await?;
    assert!(
        tx.outgoing(left.id, Some("GRM_BEHAVIOR_DELETE"))
            .await?
            .is_empty()
    );
    tx.commit().await?;

    let mut tx = backend.begin_tx().await?;
    let rel = tx
        .create_relationship(
            left.id,
            right.id,
            "GRM_BEHAVIOR_DELETE",
            props_with_run(config, []),
        )
        .await?;
    tx.delete_node(left.id).await?;
    assert!(tx.find_node_by_id(left.id).await?.is_none());
    assert!(
        tx.outgoing(left.id, Some("GRM_BEHAVIOR_DELETE"))
            .await?
            .is_empty()
    );
    assert!(
        tx.incoming(right.id, Some("GRM_BEHAVIOR_DELETE"))
            .await?
            .is_empty()
    );
    assert!(
        tx.update_relationship(rel.id, props([("after_delete", json!(true))]))
            .await?
            .is_none()
    );
    tx.commit().await?;

    let mut verify = backend.begin_tx().await?;
    assert!(verify.find_node_by_id(left.id).await?.is_none());
    assert!(
        verify
            .incoming(right.id, Some("GRM_BEHAVIOR_DELETE"))
            .await?
            .is_empty()
    );
    verify.commit().await
}

async fn transaction_visibility<B>(backend: B, config: &BackendBehaviorConfig) -> Result<()>
where
    B: GraphBackend + Clone,
{
    let mut tx = backend.begin_tx().await?;
    let created = tx
        .create_node(
            vec!["GrmBehaviorVisibility".to_string()],
            props_with_run(config, [("name", json!("Read Your Writes"))]),
        )
        .await?;
    assert!(tx.find_node_by_id(created.id).await?.is_some());
    tx.commit().await?;

    let mut later = backend.begin_tx().await?;
    let loaded = later
        .find_node_by_id(created.id)
        .await?
        .expect("committed node visible to later tx");
    assert_eq!(loaded.props.get("name"), Some(&json!("Read Your Writes")));
    later.commit().await
}

async fn rollback_discards_writes_updates_and_deletes<B>(
    backend: B,
    config: &BackendBehaviorConfig,
) -> Result<()>
where
    B: GraphBackend + Clone,
{
    let mut setup = backend.begin_tx().await?;
    let user = setup
        .create_node(
            vec!["GrmBehaviorRollbackUser".to_string()],
            props_with_run(config, [("name", json!("Original"))]),
        )
        .await?;
    let post = setup
        .create_node(
            vec!["GrmBehaviorRollbackPost".to_string()],
            props_with_run(config, []),
        )
        .await?;
    let rel = setup
        .create_relationship(
            user.id,
            post.id,
            "GRM_BEHAVIOR_ROLLBACK",
            props_with_run(config, [("weight", json!(1))]),
        )
        .await?;
    setup.commit().await?;

    let mut tx = backend.begin_tx().await?;
    let transient = tx
        .create_node(
            vec!["GrmBehaviorRollbackTransient".to_string()],
            props_with_run(config, [("name", json!("Transient"))]),
        )
        .await?;
    tx.update_node(user.id, props([("name", json!("Changed"))]))
        .await?;
    tx.update_relationship(rel.id, props([("weight", json!(99))]))
        .await?;
    tx.delete_relationship(rel.id).await?;
    tx.delete_node(post.id).await?;
    tx.rollback().await?;

    let mut verify = backend.begin_tx().await?;
    assert!(verify.find_node_by_id(transient.id).await?.is_none());
    let loaded_user = verify
        .find_node_by_id(user.id)
        .await?
        .expect("original user");
    assert_eq!(loaded_user.props.get("name"), Some(&json!("Original")));
    assert!(verify.find_node_by_id(post.id).await?.is_some());
    let outgoing = verify
        .outgoing(user.id, Some("GRM_BEHAVIOR_ROLLBACK"))
        .await?;
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].0.id, rel.id);
    assert_eq!(outgoing[0].0.props.get("weight"), Some(&json!(1)));
    verify.commit().await
}

async fn execute_graph_row_shape<B>(backend: B, config: &BackendBehaviorConfig) -> Result<()>
where
    B: GraphBackend + Clone,
{
    let (query, root, rel, end) = seed_graph_query_fixture(backend.clone(), config).await?;

    let mut tx = backend.begin_tx().await?;
    let result = tx.execute_graph(&query).await?;
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    for var in query.bound_vars() {
        assert!(row.contains_key(&var), "row missing bound var {var:?}");
    }
    assert!(
        row.get(&query.return_var()).is_some(),
        "row missing return var"
    );
    assert_eq!(query.return_kind(), ReturnKind::Node);
    match row.get(&end) {
        Some(KernelValue::Node(node)) => {
            assert_eq!(node.props.get("title"), Some(&json!("Graph Row Shape")));
        }
        other => panic!("expected returned node for {end:?}, got {other:?}"),
    }
    assert!(matches!(row.get(&root), Some(KernelValue::Node(_))));
    assert!(matches!(row.get(&rel), Some(KernelValue::Rel(_))));
    tx.commit().await
}

async fn traversal_query_returning_node_and_relationship<B>(
    backend: B,
    config: &BackendBehaviorConfig,
) -> Result<()>
where
    B: GraphBackend + Clone,
{
    let (mut node_query, _root, rel, end) =
        seed_graph_query_fixture(backend.clone(), config).await?;

    let mut tx = backend.begin_tx().await?;
    let node_result = tx.execute_graph(&node_query).await?;
    assert_eq!(node_result.rows.len(), 1);
    assert!(matches!(
        node_result.rows[0].get(&end),
        Some(KernelValue::Node(_))
    ));

    node_query.ret = Return::Rel(rel);
    let rel_result = tx.execute_graph(&node_query).await?;
    assert_eq!(rel_result.rows.len(), 1);
    assert!(matches!(
        rel_result.rows[0].get(&rel),
        Some(KernelValue::Rel(rel_value)) if rel_value.ty == "GRM_BEHAVIOR_QUERY"
    ));
    tx.commit().await
}

async fn native_query_behavior<B>(backend: B, config: &BackendBehaviorConfig) -> Result<()>
where
    B: GraphBackend + Clone,
{
    match config.native_query {
        NativeQueryExpectation::Unsupported => {
            let err = backend
                .execute_query("RETURN 1", json!({}))
                .await
                .expect_err("backend should not support native string queries");
            assert!(
                matches!(err, GrmError::Backend(_) | GrmError::NotSupported(_)),
                "expected a clear unsupported native-query error, got {err:?}"
            );
        }
        NativeQueryExpectation::Supported => {
            backend.execute_query("RETURN 1", json!({})).await?;
        }
    }
    Ok(())
}

async fn seed_graph_query_fixture<B>(
    backend: B,
    config: &BackendBehaviorConfig,
) -> Result<(GraphQuery, VarId, VarId, VarId)>
where
    B: GraphBackend + Clone,
{
    let smoke_id = unique_smoke_id();
    let mut tx = backend.begin_tx().await?;
    let user = tx
        .create_node(
            vec!["GrmBehaviorQueryUser".to_string()],
            props_with_run(
                config,
                [
                    ("name", json!("Query User")),
                    ("smoke_id", json!(smoke_id.clone())),
                ],
            ),
        )
        .await?;
    let post = tx
        .create_node(
            vec!["GrmBehaviorQueryPost".to_string()],
            props_with_run(
                config,
                [
                    ("title", json!("Graph Row Shape")),
                    ("smoke_id", json!(smoke_id.clone())),
                ],
            ),
        )
        .await?;
    tx.create_relationship(
        user.id,
        post.id,
        "GRM_BEHAVIOR_QUERY",
        props_with_run(config, [("smoke_id", json!(smoke_id.clone()))]),
    )
    .await?;
    tx.commit().await?;

    let root = VarId(0);
    let rel = VarId(1);
    let end = VarId(2);
    let query = GraphQuery {
        matches: vec![
            MatchClause::Node(NodeMatch {
                var: root,
                labels: &["GrmBehaviorQueryUser"],
                id_filter: None,
                property_filters: vec![PropertyFilter {
                    key: "smoke_id",
                    op: CompareOp::Eq,
                    value: json!(smoke_id),
                }],
            }),
            MatchClause::Hop(HopMatch {
                start: root,
                rel_type: Some("GRM_BEHAVIOR_QUERY"),
                rel_var: rel,
                dir: Direction::Out,
                end,
                end_labels: &["GrmBehaviorQueryPost"],
            }),
        ],
        where_: vec![],
        ret: Return::Node(end),
        limit: Some(1),
        offset: None,
    };

    Ok((query, root, rel, end))
}

fn props<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn props_with_run<const N: usize>(
    config: &BackendBehaviorConfig,
    entries: [(&str, Value); N],
) -> BTreeMap<String, Value> {
    let mut props = props(entries);
    props.insert(BEHAVIOR_RUN_ID_PROP.to_string(), json!(config.run_id));
    props
}

fn ids(ids: impl IntoIterator<Item = i64>) -> BTreeSet<i64> {
    ids.into_iter().collect()
}

fn unique_smoke_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("grm-rs-behavior-{nanos}")
}
