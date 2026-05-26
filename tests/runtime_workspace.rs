use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use grm_rs::{
    AdminRequest, CliSession, DefineEdgeRequest, DefineNodeRequest, DurabilityFormat,
    DurableOperation, EdgeCreateRequest, EdgeFindRequest, EdgeRequest, FieldSpec, FieldValueType,
    NodeCreateRequest, NodeFindRequest, NodeRequest, RuntimeRequest, RuntimeResponse,
    RuntimeSchemaOrigin, SchemaRequest, Workspace,
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

async fn define_workspace_schema(workspace: &mut Workspace) -> Vec<DurableOperation> {
    let mut durable_ops = Vec::new();
    durable_ops.extend(
        workspace
            .state_mut()
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
            .state_mut()
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
            .state_mut()
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
        .state_mut()
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
        .state_mut()
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
        .state_mut()
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
