use std::collections::BTreeMap;
use std::io::Cursor;

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
    let mut durable_ops = Vec::new();
    let user = workspace
        .state_mut()
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
            NodeCreateRequest {
                model: "User".to_string(),
                props: props([("name", json!("Alice"))]),
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
                props: props([("title", json!("Hello"))]),
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
                props: props([("year", json!(2026))]),
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
