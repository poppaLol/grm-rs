use std::collections::BTreeMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use grm_rs::dsl::{Direction, GraphQuery, HopMatch, MatchClause, NodeMatch, Return, VarId};
use grm_rs::{
    CompareOp, EdgeCreateRequest, EdgeFindRequest, GraphBackend, GraphClient, GraphTx,
    Neo4jBackend, Neo4jConfig, NodeCreateRequest, NodeFindRequest, NodeUpdateRequest,
    OrderDirection, OrderSpec, PredicateOp, PrimitiveKind, PropertyFilter, PropertyPredicate,
    RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeValueType,
    SessionBatchDefineNodeParams, SessionBatchFieldParam, SessionBatchNodeCreateParams,
    SessionBatchOp, SessionBatchParams, SessionBatchResponse, SessionState, apply_neo4j_batch,
    graph_query_to_cypher, neo4j_edge_create, neo4j_edge_find, neo4j_edge_update,
    neo4j_node_create, neo4j_node_find, neo4j_node_update, parse_typed_value,
};
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

#[tokio::test]
#[ignore = "requires a running Neo4j Bolt endpoint and NEO4J_* env vars"]
async fn neo4j_backend_persists_nodes_and_relationships() {
    let backend = connect_backend().await;
    let smoke_id = unique_smoke_id();
    println!("neo4j backend smoke_id={smoke_id}");

    let mut tx = backend.begin_tx().await.expect("begin Neo4j tx");
    let user = tx
        .create_node(
            vec!["GrmSmokeUser".to_string()],
            BTreeMap::from([
                ("name".to_string(), json!("Alice")),
                ("smoke_id".to_string(), json!(smoke_id.clone())),
            ]),
        )
        .await
        .expect("create user node");
    let post = tx
        .create_node(
            vec!["GrmSmokePost".to_string()],
            BTreeMap::from([
                ("title".to_string(), json!("Backend Smoke")),
                ("smoke_id".to_string(), json!(smoke_id.clone())),
            ]),
        )
        .await
        .expect("create post node");
    let rel = tx
        .create_relationship(
            user.id,
            post.id,
            "GRM_SMOKE_AUTHORED",
            BTreeMap::from([("smoke_id".to_string(), json!(smoke_id.clone()))]),
        )
        .await
        .expect("create relationship");
    tx.commit().await.expect("commit Neo4j tx");

    let mut tx = backend.begin_tx().await.expect("begin read tx");
    let loaded = tx
        .find_node_by_id(user.id)
        .await
        .expect("find user")
        .expect("user should exist");
    assert_eq!(loaded.props.get("name"), Some(&json!("Alice")));
    let outgoing = tx
        .outgoing(user.id, Some("GRM_SMOKE_AUTHORED"))
        .await
        .expect("find outgoing rels");
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].0.id, rel.id);
    assert_eq!(outgoing[0].1.id, post.id);
    tx.rollback().await.expect("rollback read tx");

    let graph = connect_graph().await;
    cleanup_smoke_graph(&graph, &smoke_id).await;
}

#[tokio::test]
#[ignore = "requires a dedicated Neo4j test endpoint and NEO4J_* env vars"]
async fn shared_neo4j_session_lists_schema_and_finds_edges() {
    let backend = connect_backend().await;
    let client = GraphClient::new(backend.clone());
    let smoke_id = unique_smoke_id();
    let mut state = SessionState::new();
    register_portable_schema(&mut state);

    assert_eq!(state.model_list().len(), 2);
    assert_eq!(state.rel_model_list().len(), 1);

    let user = neo4j_node_create(
        &client,
        &state,
        NodeCreateRequest {
            model: "PortableSmokeUser".into(),
            props: BTreeMap::from([("smoke_id".into(), json!(smoke_id.clone()))]),
        },
    )
    .await
    .expect("create portable user");
    let post = neo4j_node_create(
        &client,
        &state,
        NodeCreateRequest {
            model: "PortableSmokePost".into(),
            props: BTreeMap::from([("smoke_id".into(), json!(smoke_id.clone()))]),
        },
    )
    .await
    .expect("create portable post");
    let edge = neo4j_edge_create(
        &client,
        &state,
        EdgeCreateRequest {
            model: "PORTABLE_SMOKE_LINK".into(),
            from: user.id,
            to: post.id,
            props: BTreeMap::from([("smoke_id".into(), json!(smoke_id.clone()))]),
        },
    )
    .await
    .expect("create portable edge");

    let found_users = neo4j_node_find(
        &client,
        &state,
        NodeFindRequest::from_adapter_filter_values(
            "PortableSmokeUser",
            BTreeMap::from([("userId".into(), json!(user.id))]),
        )
        .expect("build node alias find"),
    )
    .await
    .expect("find portable user by model id alias");
    assert_eq!(found_users.len(), 1);
    assert_eq!(found_users[0].id, user.id);

    let found = neo4j_edge_find(
        &client,
        &state,
        EdgeFindRequest::from_adapter_filter_values(
            "PORTABLE_SMOKE_LINK",
            BTreeMap::from([
                ("linkId".into(), json!(edge.id)),
                ("from".into(), json!(user.id)),
                ("to".into(), json!(post.id)),
            ]),
        )
        .expect("build edge find"),
    )
    .await
    .expect("find portable edge");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, edge.id);

    backend
        .execute_query(
            "MATCH (n {smoke_id: $smoke_id}) DETACH DELETE n",
            json!({ "smoke_id": smoke_id }),
        )
        .await
        .expect("clean portable smoke graph");
}

#[tokio::test]
#[ignore = "requires a dedicated Neo4j test endpoint and NEO4J_* env vars"]
async fn shared_neo4j_atomic_batch_rolls_back_graph_and_schema() {
    let backend = connect_backend().await;
    let client = GraphClient::new(backend.clone());
    let smoke_id = unique_smoke_id();
    let mut state = SessionState::new();
    let outcome = apply_neo4j_batch(
        &client,
        &mut state,
        SessionBatchParams {
            atomic: true,
            allow_deletes: false,
            response: SessionBatchResponse::Detailed,
            ops: vec![
                SessionBatchOp::SchemaDefineNode(SessionBatchDefineNodeParams {
                    name: "RolledBackSmoke".into(),
                    id_field: "nodeId".into(),
                    fields: vec![SessionBatchFieldParam {
                        name: "smoke_id".into(),
                        value_type: "string".into(),
                        required: true,
                    }],
                }),
                SessionBatchOp::NodeCreate(SessionBatchNodeCreateParams {
                    model: "RolledBackSmoke".into(),
                    props: BTreeMap::from([("smoke_id".into(), json!(smoke_id.clone()))]),
                    local_ref: None,
                }),
                SessionBatchOp::NodeCreate(SessionBatchNodeCreateParams {
                    model: "RolledBackSmoke".into(),
                    props: BTreeMap::new(),
                    local_ref: None,
                }),
            ],
        },
    )
    .await
    .expect("apply failing atomic batch");

    assert_eq!(outcome.value["applied"], json!(false));
    assert!(outcome.schema_ops.is_empty());
    assert!(state.catalog().get_node_model("RolledBackSmoke").is_none());
    let result = backend
        .execute_query(
            "MATCH (n {smoke_id: $smoke_id}) RETURN count(n)",
            json!({ "smoke_id": smoke_id }),
        )
        .await
        .expect("count rolled-back nodes");
    let Some(grm_rs::KernelValue::Scalar(count)) = result.rows[0].values.values().next() else {
        panic!("expected scalar rollback count");
    };
    assert_eq!(count, &json!(0));
}

#[tokio::test]
#[ignore = "requires a dedicated Neo4j test endpoint and NEO4J_* env vars"]
async fn neo4j_runtime_round_trips_extended_primitives_and_queries_logically() {
    let backend = connect_backend().await;
    let client = GraphClient::new(backend.clone());
    let smoke_id = unique_smoke_id();
    let mut state = SessionState::new();
    register_primitive_schema(&mut state);

    let mut invalid_props = primitive_props(&smoke_id, "12.5", "2026-09-21T09:30:00Z", "PT1.5S");
    invalid_props.insert("amount".into(), json!("12.5"));
    let error = neo4j_node_create(
        &client,
        &state,
        NodeCreateRequest {
            model: "PrimitiveSmokeEvent".into(),
            props: invalid_props.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be decimal"));
    let error = neo4j_edge_create(
        &client,
        &state,
        EdgeCreateRequest {
            model: "PRIMITIVE_SMOKE_LINK".into(),
            from: 0,
            to: 0,
            props: invalid_props.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be decimal"));
    let error = neo4j_node_update(
        &client,
        &state,
        NodeUpdateRequest {
            model: "PrimitiveSmokeEvent".into(),
            id: 0,
            props: invalid_props.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be decimal"));
    let error = neo4j_edge_update(
        &client,
        &state,
        grm_rs::EdgeUpdateRequest {
            model: "PRIMITIVE_SMOKE_LINK".into(),
            id: 0,
            props: invalid_props,
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be decimal"));
    let error = neo4j_node_find(
        &client,
        &state,
        NodeFindRequest {
            model: "PrimitiveSmokeEvent".into(),
            predicates: vec![predicate("amount", PredicateOp::Eq, json!("12.5"))],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be decimal"));
    let error = neo4j_edge_find(
        &client,
        &state,
        EdgeFindRequest {
            model: "PRIMITIVE_SMOKE_LINK".into(),
            predicates: vec![predicate("amount", PredicateOp::Eq, json!("12.5"))],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("must be decimal"));

    let first = neo4j_node_create(
        &client,
        &state,
        NodeCreateRequest {
            model: "PrimitiveSmokeEvent".into(),
            props: primitive_props(&smoke_id, "12.5", "2026-09-21T09:30:00Z", "PT1.5S"),
        },
    )
    .await
    .expect("create first primitive node");
    assert_eq!(
        first.props["event_id"],
        typed(PrimitiveKind::Uuid, "123e4567-e89b-12d3-a456-426614174000")
    );
    assert_eq!(first.props["amount"], typed(PrimitiveKind::Decimal, "12.5"));

    let second = neo4j_node_create(
        &client,
        &state,
        NodeCreateRequest {
            model: "PrimitiveSmokeEvent".into(),
            props: primitive_props(&smoke_id, "2", "2026-09-21T09:29:00Z", "PT0.5S"),
        },
    )
    .await
    .expect("create second primitive node");

    let link = neo4j_edge_create(
        &client,
        &state,
        EdgeCreateRequest {
            model: "PRIMITIVE_SMOKE_LINK".into(),
            from: first.id,
            to: second.id,
            props: primitive_props(&smoke_id, "8.25", "2026-09-21T09:31:00Z", "PT2S"),
        },
    )
    .await
    .expect("create primitive edge");
    assert_eq!(
        link.props["duration"],
        typed(PrimitiveKind::Duration, "PT2S")
    );

    let found = neo4j_node_find(
        &client,
        &state,
        NodeFindRequest {
            model: "PrimitiveSmokeEvent".into(),
            predicates: vec![
                predicate("smoke_id", PredicateOp::Eq, json!(smoke_id.clone())),
                predicate(
                    "amount",
                    PredicateOp::Gt,
                    typed(PrimitiveKind::Decimal, "10"),
                ),
            ],
            order: vec![OrderSpec {
                field: "observed_at".into(),
                direction: OrderDirection::Asc,
            }],
            ..Default::default()
        },
    )
    .await
    .expect("find primitive node by decimal range");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, first.id);

    let ordered = neo4j_node_find(
        &client,
        &state,
        NodeFindRequest {
            model: "PrimitiveSmokeEvent".into(),
            predicates: vec![predicate(
                "smoke_id",
                PredicateOp::Eq,
                json!(smoke_id.clone()),
            )],
            order: vec![OrderSpec {
                field: "amount".into(),
                direction: OrderDirection::Asc,
            }],
            ..Default::default()
        },
    )
    .await
    .expect("order primitive nodes by decimal");
    assert_eq!(
        ordered.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![second.id, first.id]
    );

    let updated = neo4j_node_update(
        &client,
        &state,
        NodeUpdateRequest {
            model: "PrimitiveSmokeEvent".into(),
            id: first.id,
            props: BTreeMap::from([("duration".into(), typed(PrimitiveKind::Duration, "PT3S"))]),
        },
    )
    .await
    .expect("update primitive node");
    assert_eq!(
        updated.props["duration"],
        typed(PrimitiveKind::Duration, "PT3S")
    );

    let edges = neo4j_edge_find(
        &client,
        &state,
        EdgeFindRequest {
            model: "PRIMITIVE_SMOKE_LINK".into(),
            from: Some(first.id),
            predicates: vec![
                predicate("smoke_id", PredicateOp::Eq, json!(smoke_id.clone())),
                predicate(
                    "captured_on",
                    PredicateOp::Eq,
                    typed(PrimitiveKind::Date, "2026-09-21"),
                ),
            ],
            order: vec![OrderSpec {
                field: "duration".into(),
                direction: OrderDirection::Desc,
            }],
            ..Default::default()
        },
    )
    .await
    .expect("find primitive edge");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].id, link.id);

    let updated_edge = neo4j_edge_update(
        &client,
        &state,
        grm_rs::EdgeUpdateRequest {
            model: "PRIMITIVE_SMOKE_LINK".into(),
            id: link.id,
            props: BTreeMap::from([("amount".into(), typed(PrimitiveKind::Decimal, "9.75"))]),
        },
    )
    .await
    .expect("update primitive edge");
    assert_eq!(
        updated_edge.props["amount"],
        typed(PrimitiveKind::Decimal, "9.75")
    );

    backend
        .execute_query(
            "MATCH (n {smoke_id: $smoke_id}) DETACH DELETE n",
            json!({ "smoke_id": smoke_id }),
        )
        .await
        .expect("clean primitive smoke graph");
}

fn register_portable_schema(state: &mut SessionState) {
    let smoke_field = || RuntimeField {
        name: "smoke_id".into(),
        value_type: RuntimeValueType::String,
        required: true,
    };
    state
        .register_model(
            RuntimeNodeModel::new(
                "PortableSmokeUser",
                "userId",
                state.node_id_type(),
                vec![smoke_field()],
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_model(
            RuntimeNodeModel::new(
                "PortableSmokePost",
                "postId",
                state.node_id_type(),
                vec![smoke_field()],
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_rel_model(
            RuntimeRelModel::new(
                "PORTABLE_SMOKE_LINK",
                "PortableSmokeUser",
                "PortableSmokePost",
                "linkId",
                state.rel_id_type(),
                vec![smoke_field()],
            )
            .unwrap(),
        )
        .unwrap();
}

fn register_primitive_schema(state: &mut SessionState) {
    let fields = primitive_fields();
    state
        .register_model(
            RuntimeNodeModel::new(
                "PrimitiveSmokeEvent",
                "eventId",
                state.node_id_type(),
                fields.clone(),
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_rel_model(
            RuntimeRelModel::new(
                "PRIMITIVE_SMOKE_LINK",
                "PrimitiveSmokeEvent",
                "PrimitiveSmokeEvent",
                "linkId",
                state.rel_id_type(),
                fields,
            )
            .unwrap(),
        )
        .unwrap();
}

fn primitive_fields() -> Vec<RuntimeField> {
    vec![
        RuntimeField {
            name: "smoke_id".into(),
            value_type: RuntimeValueType::String,
            required: true,
        },
        RuntimeField {
            name: "payload".into(),
            value_type: RuntimeValueType::Bytes,
            required: true,
        },
        RuntimeField {
            name: "amount".into(),
            value_type: RuntimeValueType::Decimal,
            required: true,
        },
        RuntimeField {
            name: "captured_on".into(),
            value_type: RuntimeValueType::Date,
            required: true,
        },
        RuntimeField {
            name: "observed_at".into(),
            value_type: RuntimeValueType::DateTime,
            required: true,
        },
        RuntimeField {
            name: "duration".into(),
            value_type: RuntimeValueType::Duration,
            required: true,
        },
        RuntimeField {
            name: "event_id".into(),
            value_type: RuntimeValueType::Uuid,
            required: true,
        },
    ]
}

fn primitive_props(
    smoke_id: &str,
    amount: &str,
    observed_at: &str,
    duration: &str,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("smoke_id".into(), json!(smoke_id)),
        ("payload".into(), typed(PrimitiveKind::Bytes, "AQID")),
        ("amount".into(), typed(PrimitiveKind::Decimal, amount)),
        (
            "captured_on".into(),
            typed(PrimitiveKind::Date, "2026-09-21"),
        ),
        (
            "observed_at".into(),
            typed(PrimitiveKind::DateTime, observed_at),
        ),
        ("duration".into(), typed(PrimitiveKind::Duration, duration)),
        (
            "event_id".into(),
            typed(PrimitiveKind::Uuid, "123e4567-e89b-12d3-a456-426614174000"),
        ),
    ])
}

fn typed(kind: PrimitiveKind, value: &str) -> Value {
    parse_typed_value(kind, value).unwrap()
}

fn predicate(field: &str, op: PredicateOp, value: Value) -> PropertyPredicate {
    PropertyPredicate {
        field: field.into(),
        op,
        value,
    }
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

async fn connect_backend() -> Neo4jBackend {
    let uri = env::var("NEO4J_URI").unwrap_or_else(|_| "host.docker.internal:7687".to_string());
    let user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
    let password =
        env::var("NEO4J_PASSWORD").expect("set NEO4J_PASSWORD to run the Neo4j smoke test");

    Neo4jBackend::connect(Neo4jConfig {
        uri,
        user,
        password,
    })
    .await
    .expect("connect Neo4j backend")
}

async fn connect_graph() -> Graph {
    let uri = env::var("NEO4J_URI").unwrap_or_else(|_| "host.docker.internal:7687".to_string());
    let user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
    let password =
        env::var("NEO4J_PASSWORD").expect("set NEO4J_PASSWORD to run the Neo4j smoke test");

    Graph::new(&uri, &user, &password)
        .await
        .expect("connect to Neo4j")
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
