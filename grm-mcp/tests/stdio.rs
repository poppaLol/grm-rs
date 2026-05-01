use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use serde_json::{Value, json};
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

#[tokio::test]
async fn schema_list_on_empty_stdio_session() {
    let client = client(&[]).await;
    let schema = call(&client, "grm_schema_list", json!({})).await;

    assert_eq!(schema["nodes"], json!([]));
    assert_eq!(schema["edges"], json!([]));

    client.cancel().await.unwrap();
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
async fn import_find_traverse_and_export_playground() {
    let client = client(&["--import-json", "../test-dbs/query-playground.export.json"]).await;

    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": "User",
            "filters": { "name": "Alice Jones" }
        }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);

    let traversal = call(
        &client,
        "grm_query",
        json!({
            "command": "node.find User name=\"Alice Jones\" via=out:Authored:Post"
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
                "title": "MCP Note",
                "published": false
            }
        }),
    )
    .await;
    assert_eq!(created["props"]["title"], "MCP Note");

    let exported = call(&client, "grm_export", json!({ "path": null })).await;
    assert_eq!(exported["format"], "grm.interchange");
    assert!(exported["data"]["nodes"].as_array().unwrap().len() >= 8);

    client.cancel().await.unwrap();
}
