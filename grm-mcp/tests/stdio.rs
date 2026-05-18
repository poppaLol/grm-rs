use grm_rs::{DurabilityFormat, SessionState};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use serde_json::{Value, json};
use std::path::PathBuf;
use tempfile::tempdir;
use tokio::process::Command;

async fn client(args: &[&str]) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grm-mcp"));
    for arg in args {
        command.arg(arg);
    }

    ().serve(
        TokioChildProcess::new(command.configure(|cmd| {
            cmd.kill_on_drop(true);
        }))
        .expect("spawn grm-mcp"),
    )
    .await
    .expect("connect to grm-mcp")
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: Value,
) -> Value {
    let arguments: JsonObject = args.as_object().cloned().unwrap_or_default();
    let result = client
        .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(arguments))
        .await
        .expect("call tool");
    result
        .structured_content
        .expect("structured content from tool")
}

async fn call_error(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: Value,
) -> String {
    let arguments: JsonObject = args.as_object().cloned().unwrap_or_default();
    client
        .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(arguments))
        .await
        .expect_err("tool should fail")
        .to_string()
}

fn fixture_path(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn schema_list_on_empty_stdio_session() {
    let client = client(&[]).await;
    let schema = call(&client, "grm_schema_list", json!({})).await;

    assert_eq!(schema["nodes"], json!([]));
    assert_eq!(schema["edges"], json!([]));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn initialize_reports_grm_mcp_package_version() {
    let client = client(&[]).await;
    let info = client.peer_info().expect("server initialize info");

    assert_eq!(info.server_info.name, "grm-mcp");
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn schema_define_tools_expose_structured_field_objects() {
    let client = client(&[]).await;
    let tools = client.list_tools(None).await.expect("list tools");

    for tool_name in ["grm_schema_define_node", "grm_schema_define_edge"] {
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .unwrap_or_else(|| panic!("missing tool {tool_name}"));
        let fields_schema = tool
            .input_schema
            .get("properties")
            .and_then(|properties| properties.get("fields"))
            .expect("fields schema should be exposed");
        let items = fields_schema
            .get("items")
            .expect("fields should describe array items");

        assert_eq!(fields_schema["type"], json!("array"));
        assert_eq!(items["type"], json!("object"));
        assert_eq!(items["properties"]["name"]["type"], json!("string"));
        assert_eq!(items["properties"]["type"]["type"], json!("string"));
        assert_eq!(items["properties"]["required"]["type"], json!("boolean"));
    }

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn batch_tool_exposes_structured_operation_objects() {
    let client = client(&[]).await;
    let tools = client.list_tools(None).await.expect("list tools");
    let tool = tools
        .tools
        .iter()
        .find(|tool| tool.name == "grm_batch")
        .expect("missing grm_batch tool");
    let ops_schema = tool
        .input_schema
        .get("properties")
        .and_then(|properties| properties.get("ops"))
        .expect("ops schema should be exposed");
    let allow_deletes_schema = tool
        .input_schema
        .get("properties")
        .and_then(|properties| properties.get("allow_deletes"))
        .expect("allow_deletes schema should be exposed");
    let items = ops_schema
        .get("items")
        .expect("ops should describe array items");
    let variants = items
        .get("oneOf")
        .and_then(|value| value.as_array())
        .expect("batch ops should expose structured operation variants");

    assert_eq!(ops_schema["type"], json!("array"));
    assert_eq!(allow_deletes_schema["type"], json!("boolean"));
    assert_eq!(allow_deletes_schema["default"], json!(false));
    assert!(variants.iter().any(|variant| {
        variant["type"] == json!("object")
            && variant["properties"]["op"]["enum"] == json!(["node_create"])
            && variant["properties"]["args"]["properties"]["ref"]["type"] == json!("string")
    }));
    assert!(variants.iter().any(|variant| {
        variant["type"] == json!("object")
            && variant["properties"]["op"]["enum"] == json!(["edge_create"])
            && variant["properties"]["args"]["properties"]["from"]["anyOf"]
                .as_array()
                .expect("edge_create from endpoint should expose id/ref choices")
                .iter()
                .any(|choice| choice["type"] == json!("string"))
    }));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn autocommit_batch_uses_shared_wal_recovery_path() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("mcp-session.json");
    let path_arg = path.to_string_lossy().into_owned();
    let client = client(&["--autocommit-json", &path_arg]).await;

    call(
        &client,
        "grm_batch",
        json!({
            "ops": [
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "User",
                        "id_field": "userId",
                        "fields": [
                            { "name": "name", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "model": "User",
                        "props": { "name": "Alice" }
                    }
                }
            ]
        }),
    )
    .await;

    client.cancel().await.unwrap();

    let log = std::fs::read_to_string(path.with_extension("json.log")).unwrap();
    assert!(log.contains("RegisterNodeModel"));
    assert!(log.contains("UpsertNode"));

    let mut recovered = SessionState::new();
    recovered
        .recover_durable(DurabilityFormat::Json, &path)
        .unwrap();
    let nodes = recovered
        .find_nodes(
            "User",
            &std::collections::BTreeMap::from([("name".to_string(), "Alice".to_string())]),
        )
        .unwrap();
    assert_eq!(nodes.len(), 1);
}

#[tokio::test]
async fn help_tools_teach_recovery_workflow() {
    let client = client(&[]).await;

    let help = call(&client, "grm_help", json!({})).await;
    assert!(
        help["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource == "grm://docs/agent-guide")
    );

    let node_create_help = call(
        &client,
        "grm_tool_help",
        json!({ "tool": "grm_node_create" }),
    )
    .await;
    assert!(node_create_help.to_string().contains("grm_schema_list"));
    assert!(
        node_create_help
            .to_string()
            .contains("missing required field")
    );

    let batch_help = call(&client, "grm_tool_help", json!({ "tool": "grm_batch" })).await;
    assert_eq!(batch_help["defaults"]["atomic"], json!(true));
    assert_eq!(batch_help["defaults"]["allow_deletes"], json!(false));
    assert_eq!(batch_help["defaults"]["response"], json!("summary"));
    assert!(
        batch_help["supported_ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op == "edge_create")
    );
    assert!(
        batch_help["endpoint_rules"]
            .to_string()
            .contains("must be unique")
    );
    assert!(
        batch_help["result_shape"]["summary"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "counts")
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn batch_creates_connected_graph_with_refs_and_counts() {
    let client = client(&[]).await;

    let result = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "response": "detailed",
            "ops": [
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "User",
                        "id_field": "userId",
                        "fields": [
                            { "name": "name", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "Post",
                        "id_field": "postId",
                        "fields": [
                            { "name": "title", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "schema_define_edge",
                    "args": {
                        "name": "Authored",
                        "from_model": "User",
                        "to_model": "Post",
                        "id_field": "authoredId",
                        "fields": [
                            { "name": "year", "type": "int", "required": true }
                        ]
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "user:alice",
                        "model": "User",
                        "props": { "name": "Alice" }
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "post:hello",
                        "model": "Post",
                        "props": { "title": "Hello" }
                    }
                },
                {
                    "op": "edge_create",
                    "args": {
                        "model": "Authored",
                        "from": "user:alice",
                        "to": "post:hello",
                        "props": { "year": 2026 }
                    }
                }
            ]
        }),
    )
    .await;

    assert_eq!(result["applied"], true);
    assert_eq!(result["counts"]["node_create"]["User"], 1);
    assert_eq!(result["counts"]["node_create"]["Post"], 1);
    assert_eq!(result["counts"]["edge_create"]["Authored"], 1);
    assert_eq!(result["errors"], json!([]));
    assert_eq!(result["ids"].as_array().unwrap().len(), 3);

    let found = call(
        &client,
        "grm_edge_find",
        json!({
            "model": "Authored",
            "filters": { "year": 2026 }
        }),
    )
    .await;
    assert_eq!(found["edges"].as_array().unwrap().len(), 1);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn explain_and_profile_return_structured_query_introspection() {
    let import_path = fixture_path("interchange_v1_basic.json");
    let client = client(&["--import-json", &import_path]).await;

    let explain = call(
        &client,
        "grm_explain",
        json!({
            "command": "node.find User name=Alice via=out:Authored:Post"
        }),
    )
    .await;
    assert_eq!(explain["command"], "node.find");
    assert_eq!(explain["target"], "User");
    assert!(
        explain["plan"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("ExpandOut"))
    );
    assert!(
        explain["plan"]["text"]
            .as_str()
            .unwrap()
            .contains("Return Node")
    );

    let profile = call(
        &client,
        "grm_profile",
        json!({
            "command": "edge.find Authored from=1"
        }),
    )
    .await;
    assert_eq!(profile["command"], "edge.find");
    assert_eq!(profile["target"], "Authored");
    assert_eq!(profile["result_rows"], 1);
    assert!(profile["elapsed"]["micros"].as_u64().is_some());
    assert!(profile["elapsed"]["display"].as_str().is_some());
    let metrics = profile["per_step_metrics"].as_array().unwrap();
    assert!(metrics.len() >= 2);
    assert!(metrics.iter().any(|metric| {
        metric["kind"] == json!("RelationshipEndpointSeek")
            && metric["access_path"] == json!("outgoing_adjacency")
            && metric["input_rows"] == json!(0)
            && metric["output_rows"] == json!(1)
            && metric["elapsed_micros"].as_u64().is_some()
    }));
    assert!(metrics.iter().any(|metric| {
        metric["kind"] == json!("Return")
            && metric["input_rows"] == json!(1)
            && metric["output_rows"] == json!(1)
            && metric["elapsed_micros"].as_u64().is_some()
    }));

    let error = call_error(
        &client,
        "grm_explain",
        json!({
            "command": "node.find User format=jsonl"
        }),
    )
    .await;
    assert!(error.contains("format= is not supported with session.explain or session.profile"));

    let wrong_prefix = call_error(
        &client,
        "grm_profile",
        json!({
            "command": "session.explain node.find User name=Alice"
        }),
    )
    .await;
    assert!(wrong_prefix.contains("expected session.profile command"));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn failed_atomic_batch_leaves_session_unchanged() {
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;

    let result = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "ops": [
                {
                    "op": "node_create",
                    "args": {
                        "model": "Note",
                        "props": { "title": "Kept only if batch succeeds" }
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "model": "Note",
                        "props": {}
                    }
                }
            ]
        }),
    )
    .await;

    assert_eq!(result["applied"], false);
    assert_eq!(result["errors"][0]["index"], 1);

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "title": "Kept only if batch succeeds" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 0);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn duplicate_batch_refs_are_rejected_and_atomic_batch_rolls_back() {
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;

    let result = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "ops": [
                {
                    "op": "node_create",
                    "args": {
                        "ref": "note:duplicate",
                        "model": "Note",
                        "props": { "title": "First duplicate ref" }
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "note:duplicate",
                        "model": "Note",
                        "props": { "title": "Second duplicate ref" }
                    }
                }
            ]
        }),
    )
    .await;

    assert_eq!(result["applied"], false);
    assert_eq!(result["errors"][0]["index"], 1);
    assert!(
        result["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("duplicate batch ref")
    );

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "title": "First duplicate ref" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 0);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn batch_deletes_require_explicit_allow_deletes() {
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    let created = call(
        &client,
        "grm_node_create",
        json!({
            "model": "Note",
            "props": { "title": "Delete only when allowed" }
        }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let rejected = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "ops": [
                {
                    "op": "node_delete",
                    "args": { "model": "Note", "id": id }
                }
            ]
        }),
    )
    .await;
    assert_eq!(rejected["applied"], false);
    assert!(
        rejected["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("requires allow_deletes=true")
    );

    let still_found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "id": id }
        }),
    )
    .await;
    assert_eq!(still_found["nodes"].as_array().unwrap().len(), 1);

    let allowed = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "allow_deletes": true,
            "ops": [
                {
                    "op": "node_delete",
                    "args": { "model": "Note", "id": id }
                }
            ]
        }),
    )
    .await;
    assert_eq!(allowed["applied"], true);
    assert_eq!(allowed["counts"]["node_delete"]["Note"], 1);

    let gone = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "id": id }
        }),
    )
    .await;
    assert_eq!(gone["nodes"].as_array().unwrap().len(), 0);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn non_atomic_batch_reports_partial_success() {
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;

    let result = call(
        &client,
        "grm_batch",
        json!({
            "atomic": false,
            "ops": [
                {
                    "op": "node_create",
                    "args": {
                        "model": "Note",
                        "props": { "title": "Partial success" }
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "model": "Note",
                        "props": {}
                    }
                }
            ]
        }),
    )
    .await;

    assert_eq!(result["applied"], false);
    assert_eq!(result["atomic"], false);
    assert_eq!(result["counts"]["node_create"]["Note"], 1);
    assert_eq!(result["errors"][0]["index"], 1);

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "title": "Partial success" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn export_json_flag_writes_readable_graph_after_mutation() {
    let temp = tempdir().unwrap();
    let export_path = temp.path().join("graph.export.json");
    let export_path_arg = export_path.to_string_lossy().to_string();
    let client = client(&["--export-json", &export_path_arg]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    call(
        &client,
        "grm_node_create",
        json!({
            "model": "Note",
            "props": { "title": "export smoke" }
        }),
    )
    .await;

    let exported: Value =
        serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
    assert_eq!(exported["format"], "grm.interchange");
    assert_eq!(
        exported["data"]["nodes"][0]["props"]["title"],
        "export smoke"
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn failed_grm_query_preserves_stdio_session_state() {
    let import_path = fixture_path("interchange_v1_basic.json");
    let client = client(&["--import-json", &import_path]).await;

    let error = call_error(
        &client,
        "grm_query",
        json!({
            "command": "node.find MissingModel name=\"Alice\""
        }),
    )
    .await;
    assert!(!error.is_empty());

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "User",
            "filters": { "name": "Alice" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn failed_structured_mutation_preserves_stdio_session_state() {
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;

    let error = call_error(
        &client,
        "grm_node_create",
        json!({
            "model": "Note",
            "props": {}
        }),
    )
    .await;
    assert!(!error.is_empty());

    let created = call(
        &client,
        "grm_node_create",
        json!({
            "model": "Note",
            "props": { "title": "After failed create" }
        }),
    )
    .await;
    assert_eq!(created["props"]["title"], "After failed create");

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "title": "After failed create" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn failed_import_into_non_empty_session_preserves_existing_graph() {
    let import_path = fixture_path("interchange_v1_basic.json");
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Note",
            "id_field": "noteId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    call(
        &client,
        "grm_node_create",
        json!({
            "model": "Note",
            "props": { "title": "Keep me" }
        }),
    )
    .await;

    let error = call_error(
        &client,
        "grm_import",
        json!({
            "path": import_path
        }),
    )
    .await;
    assert!(!error.is_empty());

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "Note",
            "filters": { "title": "Keep me" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn import_find_traverse_and_export_basic_interchange() {
    let import_path = fixture_path("interchange_v1_basic.json");
    let client = client(&["--import-json", &import_path]).await;

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "User",
            "filters": { "name": "Alice" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);

    let traversal = call(
        &client,
        "grm_query",
        json!({
            "command": "node.find User name=\"Alice\" via=out:Authored:Post"
        }),
    )
    .await;
    assert!(traversal["output"].as_str().unwrap().contains("Post"));

    let created = call(
        &client,
        "grm_node_create",
        json!({
            "model": "Post",
            "props": {
                "title": "MCP Note"
            }
        }),
    )
    .await;
    assert_eq!(created["props"]["title"], "MCP Note");

    let exported = call(&client, "grm_export", json!({ "path": null })).await;
    assert_eq!(exported["format"], "grm.interchange");
    assert_eq!(exported["data"]["nodes"].as_array().unwrap().len(), 3);

    client.cancel().await.unwrap();
}
