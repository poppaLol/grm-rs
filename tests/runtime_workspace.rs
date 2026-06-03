use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::io::Write;
use std::path::{Path, PathBuf};

use grm_rs::{
    AdminRequest, BatchRequest, CliSession, DefineEdgeRequest, DefineNodeRequest, DurabilityFormat,
    DurableOperation, EdgeCreateRequest, EdgeDeleteRequest, EdgeFindRequest, EdgeRequest,
    EdgeUpdateRequest, FieldSpec, FieldValueType, NodeCreateRequest, NodeDeleteRequest,
    NodeFindRequest, NodeRequest, NodeUpdateRequest, RuntimeRequest, RuntimeResponse,
    RuntimeSchemaOrigin, SchemaRequest, SessionBatchEdgeCreateParams, SessionBatchEndpoint,
    SessionBatchNodeCreateParams, SessionBatchOp, SessionBatchResponse, Workspace,
};
use serde_json::{Value, json};
use tempfile::tempdir;

#[tokio::test]
async fn workspace_json_closed_loop_preserves_declared_schema() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    let mut workspace = Workspace::new();

    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    workspace.save(DurabilityFormat::Json, &path).unwrap();

    let reopened = Workspace::load(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_data_survived(&reopened).await;
}

#[tokio::test]
async fn workspace_binary_closed_loop_preserves_declared_schema() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("workspace.bin");
    let mut workspace = Workspace::new();

    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    workspace.save(DurabilityFormat::Binary, &path).unwrap();

    let reopened = Workspace::open(DurabilityFormat::Binary, &path).unwrap();
    assert_declared_schema_and_data_survived(&reopened).await;
}

#[tokio::test]
async fn workspace_load_replays_durable_log_entries() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("workspace-with-log.json");
    let mut workspace = Workspace::new();
    workspace.save(DurabilityFormat::Json, &path).unwrap();

    let mut durable_ops = define_workspace_schema(&mut workspace).await;
    durable_ops.extend(create_workspace_data(&mut workspace).await);
    for op in durable_ops {
        workspace
            .state()
            .append_durable_operation(&path, &op)
            .unwrap();
    }

    let reopened = Workspace::load(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_data_survived(&reopened).await;
}

#[tokio::test]
async fn workspace_binary_load_replays_durable_log_entries() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("workspace-with-log.bin");
    let mut workspace = Workspace::new();
    workspace.save(DurabilityFormat::Binary, &path).unwrap();

    let mut durable_ops = define_workspace_schema(&mut workspace).await;
    durable_ops.extend(create_workspace_data(&mut workspace).await);
    for op in durable_ops {
        workspace
            .state()
            .append_durable_operation(&path, &op)
            .unwrap();
    }
    assert!(
        fs::metadata(log_path(&path)).unwrap().len() > 0,
        "binary checkpoint replay should use the same durable append log"
    );

    let reopened = Workspace::load(DurabilityFormat::Binary, &path).unwrap();
    assert_declared_schema_and_data_survived(&reopened).await;
}

#[tokio::test]
async fn workspace_repeated_closed_loop_reload_preserves_schema_and_data() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("repeated-workspace.json");
    let mut workspace = Workspace::new();

    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    workspace.save(DurabilityFormat::Json, &path).unwrap();
    assert!(
        !log_path(&path).exists(),
        "checkpoint should clear durable append log"
    );

    let mut reopened = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_data_survived(&reopened).await;

    create_workspace_data_named(&mut reopened, "Bob", "Reloaded", 2027).await;
    reopened.save(DurabilityFormat::Json, &path).unwrap();
    assert!(
        !log_path(&path).exists(),
        "second checkpoint should clear durable append log"
    );

    let reopened_again = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_all_data_survived(
        &reopened_again,
        ["Alice", "Bob"],
        ["Hello", "Reloaded"],
        [2026, 2027],
    )
    .await;
}

#[tokio::test]
async fn workspace_reopen_mutate_checkpoint_reopen_preserves_declared_schema() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("checkpoint-after-reopen.bin");
    let mut workspace = Workspace::new();

    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    workspace
        .checkpoint(DurabilityFormat::Binary, &path)
        .unwrap();

    let mut reopened = Workspace::open(DurabilityFormat::Binary, &path).unwrap();
    create_workspace_data_named(&mut reopened, "Bob", "Reloaded", 2027).await;
    reopened
        .checkpoint(DurabilityFormat::Binary, &path)
        .unwrap();

    let reopened_again = Workspace::open(DurabilityFormat::Binary, &path).unwrap();
    assert_declared_schema_and_all_data_survived(
        &reopened_again,
        ["Alice", "Bob"],
        ["Hello", "Reloaded"],
        [2026, 2027],
    )
    .await;
}

#[tokio::test]
async fn workspace_recovery_replays_log_after_reopened_workspace() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("reopened-with-log.json");
    let mut workspace = Workspace::new();

    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    workspace.save(DurabilityFormat::Json, &path).unwrap();

    let mut reopened = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    let later_ops = create_workspace_data_named(&mut reopened, "Bob", "Logged", 2027).await;
    for op in later_ops {
        reopened
            .state()
            .append_durable_operation(&path, &op)
            .unwrap();
    }
    assert!(
        fs::metadata(log_path(&path)).unwrap().len() > 0,
        "mutations after reopen should be represented in the durable append log"
    );

    let recovered = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_all_data_survived(
        &recovered,
        ["Alice", "Bob"],
        ["Hello", "Logged"],
        [2026, 2027],
    )
    .await;

    recovered.checkpoint(DurabilityFormat::Json, &path).unwrap();
    assert!(
        !log_path(&path).exists(),
        "checkpoint after recovery should fold replayed log data into the snapshot"
    );

    let checkpointed = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_all_data_survived(
        &checkpointed,
        ["Alice", "Bob"],
        ["Hello", "Logged"],
        [2026, 2027],
    )
    .await;
}

#[tokio::test]
async fn workspace_execute_runtime_autocommits_to_reopenable_workspace() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("autocommit-workspace.json");
    let mut workspace = Workspace::new();

    workspace
        .enable_autocommit(DurabilityFormat::Json, &path)
        .unwrap();
    assert!(
        path.exists(),
        "enabling autocommit should checkpoint the current workspace"
    );
    assert!(
        !log_path(&path).exists(),
        "enabling autocommit should not create append records"
    );

    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    assert!(
        fs::metadata(log_path(&path)).unwrap().len() > 0,
        "workspace runtime mutations should append durable records"
    );

    let find = workspace
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Find(NodeFindRequest {
            model: "User".to_string(),
            ..Default::default()
        })))
        .await
        .unwrap();
    assert!(find.durable_ops.is_empty());
    let log_len_after_find = fs::metadata(log_path(&path)).unwrap().len();

    let reopened = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_data_survived(&reopened).await;
    assert_eq!(
        fs::metadata(log_path(&path)).unwrap().len(),
        log_len_after_find,
        "read-only runtime requests should not append durable records"
    );
}

#[tokio::test]
async fn workspace_autocommit_checkpoints_after_interval_then_replays_later_log() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("autocommit-threshold-workspace.json");
    let mut workspace = Workspace::new();

    workspace
        .enable_autocommit(DurabilityFormat::Json, &path)
        .unwrap();
    define_workspace_schema(&mut workspace).await; // 3 durable records.
    create_workspace_data(&mut workspace).await; // 6 durable records total.
    create_workspace_node(&mut workspace, "User", props([("name", json!("Bob"))])).await;
    create_workspace_node(
        &mut workspace,
        "Post",
        props([("title", json!("Threshold"))]),
    )
    .await;

    assert!(
        !log_path(&path).exists(),
        "the eighth durable record should checkpoint and clear the append log"
    );

    let checkpointed = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_workspace_counts(&checkpointed, 2, 2, 1).await;

    let bob_id = find_one_node_id(&checkpointed, "User", "name", "Bob").await;
    let threshold_post_id = find_one_node_id(&checkpointed, "Post", "title", "Threshold").await;
    workspace
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Create(
            EdgeCreateRequest {
                model: "Authored".to_string(),
                from: bob_id,
                to: threshold_post_id,
                props: props([("year", json!(2027))]),
            },
        )))
        .await
        .unwrap();
    assert!(
        fs::metadata(log_path(&path)).unwrap().len() > 0,
        "records after the checkpoint interval should start a new append log"
    );

    let recovered = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_all_data_survived(
        &recovered,
        ["Alice", "Bob"],
        ["Hello", "Threshold"],
        [2026, 2027],
    )
    .await;
}

#[tokio::test]
async fn workspace_reopen_ignores_truncated_final_autocommit_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("truncated-final-record.json");
    let mut workspace = Workspace::new();

    workspace
        .enable_autocommit(DurabilityFormat::Json, &path)
        .unwrap();
    define_workspace_schema(&mut workspace).await;
    create_workspace_data(&mut workspace).await;
    fs::OpenOptions::new()
        .append(true)
        .open(log_path(&path))
        .unwrap()
        .write_all(br#"{"UpsertNode":"#)
        .unwrap();

    let recovered = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_data_survived(&recovered).await;
}

#[tokio::test]
async fn workspace_reopen_rejects_malformed_complete_autocommit_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("malformed-complete-record.json");
    let mut workspace = Workspace::new();

    workspace
        .enable_autocommit(DurabilityFormat::Json, &path)
        .unwrap();
    define_workspace_schema(&mut workspace).await;
    fs::OpenOptions::new()
        .append(true)
        .open(log_path(&path))
        .unwrap()
        .write_all(b"{bad json}\n")
        .unwrap();

    let err = match Workspace::open(DurabilityFormat::Json, &path) {
        Ok(_) => panic!("malformed complete append-log record should abort workspace open"),
        Err(err) => err,
    };
    assert!(
        err.to_string()
            .contains("malformed durable append log record"),
        "{err}"
    );
}

#[tokio::test]
async fn workspace_replay_covers_schema_crud_edge_crud_and_grouped_batch_ops() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("crud-and-batch-replay.json");
    let mut workspace = Workspace::new();

    workspace
        .enable_autocommit(DurabilityFormat::Json, &path)
        .unwrap();
    define_workspace_schema(&mut workspace).await;
    let alice_ops = create_workspace_data(&mut workspace).await;
    assert_eq!(alice_ops.len(), 3);
    let bob_ops = create_workspace_data_named(&mut workspace, "Bob", "Draft", 2027).await;
    assert_eq!(bob_ops.len(), 3);

    let alice_id = find_one_node_id(&workspace, "User", "name", "Alice").await;
    let draft_post_id = find_one_node_id(&workspace, "Post", "title", "Draft").await;
    let alice_edge_id = find_one_edge_id(&workspace, 2026);
    let bob_edge_id = find_one_edge_id(&workspace, 2027);

    workspace
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Update(
            NodeUpdateRequest {
                model: "User".to_string(),
                id: alice_id,
                props: props([("name", json!("Ada"))]),
            },
        )))
        .await
        .unwrap();
    workspace
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Update(
            EdgeUpdateRequest {
                model: "Authored".to_string(),
                id: alice_edge_id,
                props: props([("year", json!(2028))]),
            },
        )))
        .await
        .unwrap();
    workspace
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Delete(
            EdgeDeleteRequest {
                model: "Authored".to_string(),
                id: bob_edge_id,
            },
        )))
        .await
        .unwrap();
    workspace
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Delete(
            NodeDeleteRequest {
                model: "Post".to_string(),
                id: draft_post_id,
            },
        )))
        .await
        .unwrap();

    let batch = workspace
        .execute_runtime(RuntimeRequest::Batch(BatchRequest {
            atomic: true,
            allow_deletes: false,
            response: SessionBatchResponse::Detailed,
            ops: vec![
                SessionBatchOp::NodeCreate(SessionBatchNodeCreateParams {
                    model: "User".to_string(),
                    props: props([("name", json!("Carol"))]),
                    local_ref: Some("carol".to_string()),
                }),
                SessionBatchOp::NodeCreate(SessionBatchNodeCreateParams {
                    model: "Post".to_string(),
                    props: props([("title", json!("Batch"))]),
                    local_ref: Some("batch_post".to_string()),
                }),
                SessionBatchOp::EdgeCreate(SessionBatchEdgeCreateParams {
                    model: "Authored".to_string(),
                    from: SessionBatchEndpoint::Ref("carol".to_string()),
                    to: SessionBatchEndpoint::Ref("batch_post".to_string()),
                    props: props([("year", json!(2029))]),
                }),
            ],
        }))
        .await
        .unwrap();
    assert!(matches!(
        batch.durable_ops.as_slice(),
        [DurableOperation::Batch { ops }] if ops.len() == 3
    ));

    let recovered = Workspace::open(DurabilityFormat::Json, &path).unwrap();
    assert_declared_schema_and_all_data_survived(
        &recovered,
        ["Ada", "Bob", "Carol"],
        ["Hello", "Batch"],
        [2028, 2029],
    )
    .await;
}

#[tokio::test]
async fn cli_save_load_uses_workspace_schema_snapshot() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cli-workspace.json");
    let path_arg = path.display();

    let create_script = format!(
        "model.define User userId name:string:required email:string:optional\n\
         model.define Post postId title:string:required\n\
         link.define Authored User Post authoredId year:int:required role:string:optional\n\
         node.create User name=Alice\n\
         node.create Post title=Hello\n\
         edge.create Authored from=1 to=2 year=2026\n\
         session.save --json {path_arg}\n\
         session.exit\n"
    );
    let output = Vec::new();
    let mut writer = CliSession::new(Cursor::new(create_script), output);
    writer.run_script().await.unwrap();

    let load_script = format!(
        "session.load --json {path_arg}\n\
         model.show User\n\
         link.show Authored\n\
         node.find User name=Alice\n\
         session.exit\n"
    );
    let output = Vec::new();
    let mut reader = CliSession::new(Cursor::new(load_script), output);
    reader.run().await.unwrap();
    let (_, _, output) = reader.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Loaded session from JSON file"), "{output}");
    assert!(output.contains("email: string (optional)"), "{output}");
    assert!(output.contains("role: string (optional)"), "{output}");
    assert!(output.contains("Alice"), "{output}");
}

async fn create_workspace_node(
    workspace: &mut Workspace,
    model: &'static str,
    props: BTreeMap<String, Value>,
) -> i64 {
    let outcome = workspace
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
            NodeCreateRequest {
                model: model.to_string(),
                props,
            },
        )))
        .await
        .unwrap();
    let RuntimeResponse::Node(grm_rs::NodeResponse::Create(node)) = outcome.response else {
        panic!("expected node create response");
    };
    node.id
}

async fn define_workspace_schema(workspace: &mut Workspace) -> Vec<DurableOperation> {
    let mut durable_ops = Vec::new();
    durable_ops.extend(
        workspace
            .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
                DefineNodeRequest {
                    name: "User".to_string(),
                    id_field: "userId".to_string(),
                    fields: vec![
                        field("name", FieldValueType::String, true),
                        field("email", FieldValueType::String, false),
                    ],
                },
            )))
            .await
            .unwrap()
            .durable_ops,
    );
    durable_ops.extend(
        workspace
            .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
                DefineNodeRequest {
                    name: "Post".to_string(),
                    id_field: "postId".to_string(),
                    fields: vec![field("title", FieldValueType::String, true)],
                },
            )))
            .await
            .unwrap()
            .durable_ops,
    );
    durable_ops.extend(
        workspace
            .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineEdge(
                DefineEdgeRequest {
                    name: "Authored".to_string(),
                    from_model: "User".to_string(),
                    to_model: "Post".to_string(),
                    id_field: "authoredId".to_string(),
                    fields: vec![
                        field("year", FieldValueType::Int, true),
                        field("role", FieldValueType::String, false),
                    ],
                },
            )))
            .await
            .unwrap()
            .durable_ops,
    );
    durable_ops
}

async fn assert_workspace_counts(
    workspace: &Workspace,
    expected_users: usize,
    expected_posts: usize,
    expected_edges: usize,
) {
    let users = workspace
        .state()
        .node_find_response(NodeFindRequest {
            model: "User".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(users.nodes.len(), expected_users);

    let posts = workspace
        .state()
        .node_find_response(NodeFindRequest {
            model: "Post".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(posts.nodes.len(), expected_posts);

    let edges = workspace
        .state()
        .edge_find_response(EdgeFindRequest {
            model: "Authored".to_string(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(edges.edges.len(), expected_edges);
}

async fn find_one_node_id(
    workspace: &Workspace,
    model: &'static str,
    field: &'static str,
    value: &'static str,
) -> i64 {
    let response = workspace
        .state()
        .node_find_response(NodeFindRequest {
            model: model.to_string(),
            predicates: vec![grm_rs::PropertyPredicate {
                field: field.to_string(),
                op: grm_rs::PredicateOp::Eq,
                value: json!(value),
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.nodes.len(), 1);
    response.nodes[0].id
}

fn find_one_edge_id(workspace: &Workspace, year: i64) -> i64 {
    let response = workspace
        .state()
        .edge_find_response(EdgeFindRequest {
            model: "Authored".to_string(),
            predicates: vec![grm_rs::PropertyPredicate {
                field: "year".to_string(),
                op: grm_rs::PredicateOp::Eq,
                value: json!(year),
            }],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(response.edges.len(), 1);
    response.edges[0].id
}

async fn create_workspace_data(workspace: &mut Workspace) -> Vec<DurableOperation> {
    create_workspace_data_named(workspace, "Alice", "Hello", 2026).await
}

async fn create_workspace_data_named(
    workspace: &mut Workspace,
    user_name: &'static str,
    post_title: &'static str,
    year: i64,
) -> Vec<DurableOperation> {
    let mut durable_ops = Vec::new();
    let user = workspace
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
            NodeCreateRequest {
                model: "User".to_string(),
                props: props([("name", json!(user_name))]),
            },
        )))
        .await
        .unwrap();
    durable_ops.extend(user.durable_ops.clone());
    let post = workspace
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
            NodeCreateRequest {
                model: "Post".to_string(),
                props: props([("title", json!(post_title))]),
            },
        )))
        .await
        .unwrap();
    durable_ops.extend(post.durable_ops.clone());

    let RuntimeResponse::Node(grm_rs::NodeResponse::Create(user)) = user.response else {
        panic!("expected user create response");
    };
    let RuntimeResponse::Node(grm_rs::NodeResponse::Create(post)) = post.response else {
        panic!("expected post create response");
    };

    let edge = workspace
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Create(
            EdgeCreateRequest {
                model: "Authored".to_string(),
                from: user.id,
                to: post.id,
                props: props([("year", json!(year))]),
            },
        )))
        .await
        .unwrap();
    durable_ops.extend(edge.durable_ops);
    durable_ops
}

async fn assert_declared_schema_and_data_survived(workspace: &Workspace) {
    let schema = workspace.state().admin(AdminRequest::SchemaList).unwrap();
    assert_eq!(schema["nodes"][0]["origin"], json!("declared"));
    assert_eq!(schema["edges"][0]["origin"], json!("declared"));

    let user = workspace.state().model("User").unwrap();
    assert_eq!(user.origin, RuntimeSchemaOrigin::Declared);
    assert!(user.field("email").is_some());

    let authored = workspace.state().rel_model("Authored").unwrap();
    assert_eq!(authored.origin, RuntimeSchemaOrigin::Declared);
    assert!(authored.field("role").is_some());

    let users = workspace
        .state()
        .node_find_response(NodeFindRequest {
            model: "User".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(users.nodes.len(), 1);
    assert_eq!(users.nodes[0].props.get("name"), Some(&json!("Alice")));

    let edges = workspace
        .state()
        .edge_find_response(EdgeFindRequest {
            model: "Authored".to_string(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(edges.edges.len(), 1);
    assert_eq!(edges.edges[0].props.get("year"), Some(&json!(2026)));
}

async fn assert_declared_schema_and_all_data_survived(
    workspace: &Workspace,
    expected_users: impl IntoIterator<Item = &'static str>,
    expected_posts: impl IntoIterator<Item = &'static str>,
    expected_years: impl IntoIterator<Item = i64>,
) {
    let schema = workspace.state().admin(AdminRequest::SchemaList).unwrap();
    assert_eq!(schema["nodes"][0]["origin"], json!("declared"));
    assert_eq!(schema["edges"][0]["origin"], json!("declared"));

    let user = workspace.state().model("User").unwrap();
    assert_eq!(user.origin, RuntimeSchemaOrigin::Declared);
    assert!(user.field("email").is_some());

    let authored = workspace.state().rel_model("Authored").unwrap();
    assert_eq!(authored.origin, RuntimeSchemaOrigin::Declared);
    assert!(authored.field("role").is_some());

    let users = workspace
        .state()
        .node_find_response(NodeFindRequest {
            model: "User".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut user_names = users
        .nodes
        .iter()
        .map(|node| node.props["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    user_names.sort();
    let mut expected_user_names = expected_users
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    expected_user_names.sort();
    assert_eq!(user_names, expected_user_names);

    let posts = workspace
        .state()
        .node_find_response(NodeFindRequest {
            model: "Post".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut post_titles = posts
        .nodes
        .iter()
        .map(|node| node.props["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    post_titles.sort();
    let mut expected_post_titles = expected_posts
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    expected_post_titles.sort();
    assert_eq!(post_titles, expected_post_titles);

    let edges = workspace
        .state()
        .edge_find_response(EdgeFindRequest {
            model: "Authored".to_string(),
            ..Default::default()
        })
        .unwrap();
    let mut years = edges
        .edges
        .iter()
        .map(|edge| edge.props["year"].as_i64().unwrap())
        .collect::<Vec<_>>();
    years.sort();
    let mut expected_edge_years = expected_years.into_iter().collect::<Vec<_>>();
    expected_edge_years.sort();
    assert_eq!(years, expected_edge_years);
}

fn field(name: &str, value_type: FieldValueType, required: bool) -> FieldSpec {
    FieldSpec {
        name: name.to_string(),
        value_type,
        required,
    }
}

fn props(entries: impl IntoIterator<Item = (&'static str, Value)>) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn log_path(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("grm-data");

    parent.join(format!("{file_name}.log"))
}
