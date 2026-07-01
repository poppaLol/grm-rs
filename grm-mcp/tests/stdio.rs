use grm_rs::{DurabilityFormat, GraphBackend, Neo4jBackend, Neo4jConfig, SessionState};
use grm_service_api::GrpcWorkspaceService;
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, JsonObject, ListToolsResult, ReadResourceRequestParams, ResourceContents,
};
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use serde_json::{Value, json};
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

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

async fn http_client() -> (
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tokio::process::Child,
) {
    let probe = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral HTTP MCP port");
    let addr = probe.local_addr().expect("ephemeral HTTP MCP addr");
    drop(probe);

    let mut child = Command::new(env!("CARGO_BIN_EXE_grm-mcp"))
        .arg("--transport")
        .arg("http")
        .arg("--http-bind")
        .arg(addr.to_string())
        .arg("--http-path")
        .arg("/mcp")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn grm-mcp HTTP server");

    let uri = format!("http://{addr}/mcp");
    for _ in 0..20 {
        match ().serve(StreamableHttpClientTransport::from_uri(uri.clone())).await {
            Ok(client) => return (client, child),
            Err(_) => {
                if let Some(status) = child.try_wait().expect("poll HTTP MCP child") {
                    panic!("grm-mcp HTTP server exited before accepting clients: {status}");
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
    child
        .kill()
        .await
        .expect("kill unresponsive HTTP MCP child");
    panic!("grm-mcp HTTP server did not accept clients on {uri}");
}

async fn neo4j_client() -> Option<rmcp::service::RunningService<rmcp::RoleClient, ()>> {
    let uri = env::var("NEO4J_URI").ok()?;
    let user = env::var("NEO4J_USER").ok()?;
    let password = env::var("NEO4J_PASSWORD").ok()?;
    let command = Command::new(env!("CARGO_BIN_EXE_grm-mcp"));

    Some(
        ().serve(
            TokioChildProcess::new(command.configure(|cmd| {
                cmd.kill_on_drop(true)
                    .env("GRM_BACKEND", "neo4j")
                    .env("NEO4J_URI", uri)
                    .env("NEO4J_USER", user)
                    .env("NEO4J_PASSWORD", password);
            }))
            .expect("spawn grm-mcp in Neo4j mode"),
        )
        .await
        .expect("connect to grm-mcp"),
    )
}

async fn grpc_service(root: PathBuf) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local gRPC service");
    let addr = listener.local_addr().expect("local gRPC service address");
    let incoming = TcpListenerStream::new(listener);
    let service = GrpcWorkspaceService::with_local_workspace_root(
        root,
        grm_service_api::ServiceSecurityConfig::anonymous_local(),
    )
    .into_server();
    let handle = tokio::spawn(async move {
        Server::builder()
            .add_service(service)
            .serve_with_incoming(incoming)
            .await
            .expect("serve local gRPC workspace service");
    });
    (format!("http://{addr}"), handle)
}

async fn grpc_mcp_client(
    endpoint: &str,
    workspace_ref: &str,
    mode: &str,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    grpc_mcp_client_with_format(endpoint, workspace_ref, mode, None).await
}

async fn grpc_mcp_client_with_format(
    endpoint: &str,
    workspace_ref: &str,
    mode: &str,
    format: Option<&str>,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let command = Command::new(env!("CARGO_BIN_EXE_grm-mcp"));

    ().serve(
        TokioChildProcess::new(command.configure(|cmd| {
            cmd.kill_on_drop(true)
                .env("GRM_BACKEND", "grpc")
                .env("GRM_SERVICE_ENDPOINT", endpoint)
                .env("GRM_WORKSPACE_REF", workspace_ref)
                .env("GRM_SERVICE_WORKSPACE_MODE", mode);
            if let Some(format) = format {
                cmd.env("GRM_SERVICE_WORKSPACE_FORMAT", format);
            }
        }))
        .expect("spawn grm-mcp in gRPC service mode"),
    )
    .await
    .expect("connect to grm-mcp in gRPC service mode")
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

fn assert_read_only_tool_annotations(tools: &ListToolsResult) {
    for tool_name in [
        "grm_help",
        "grm_tool_help",
        "grm_schema_list",
        "grm_index_list",
        "grm_node_find",
        "grm_edge_find",
        "grm_explain",
        "grm_profile",
    ] {
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name.as_ref() == tool_name)
            .unwrap_or_else(|| panic!("missing tool {tool_name}"));
        let annotations = tool
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("missing annotations for {tool_name}"));

        assert_eq!(
            annotations.read_only_hint,
            Some(true),
            "{tool_name} should advertise read-only behavior"
        );
        assert_eq!(
            annotations.open_world_hint,
            Some(false),
            "{tool_name} should advertise a closed-world graph scope"
        );
    }

    for tool_name in [
        "grm_help",
        "grm_tool_help",
        "grm_schema_list",
        "grm_explain",
    ] {
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name.as_ref() == tool_name)
            .unwrap_or_else(|| panic!("missing tool {tool_name}"));
        assert_eq!(
            tool.annotations
                .as_ref()
                .and_then(|annotations| annotations.idempotent_hint),
            Some(true),
            "{tool_name} should advertise idempotent behavior"
        );
    }

    let query = tools
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == "grm_query")
        .expect("missing grm_query tool");
    assert_ne!(
        query
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint),
        Some(true),
        "grm_query should not be advertised as read-only until it is constrained to read commands"
    );
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

fn unique_smoke_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    format!("grm-mcp-neo4j-batch-{nanos}")
}

async fn cleanup_neo4j_smoke_graph(smoke_id: &str) {
    let Ok(uri) = env::var("NEO4J_URI") else {
        return;
    };
    let Ok(user) = env::var("NEO4J_USER") else {
        return;
    };
    let Ok(password) = env::var("NEO4J_PASSWORD") else {
        return;
    };
    let backend = Neo4jBackend::connect(Neo4jConfig {
        uri,
        user,
        password,
    })
    .await
    .expect("connect Neo4j for smoke cleanup");
    backend
        .execute_query(
            "MATCH (n) WHERE n.smoke_id = $smoke_id DETACH DELETE n",
            json!({ "smoke_id": smoke_id }),
        )
        .await
        .expect("cleanup Neo4j MCP smoke graph");
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
async fn streamable_http_initializes_lists_tools_and_calls_schema_list() {
    let (client, mut server) = http_client().await;

    let tools = client
        .peer()
        .list_tools(Default::default())
        .await
        .expect("list tools over Streamable HTTP");
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name.as_ref() == "grm_schema_list")
    );

    let schema = call(&client, "grm_schema_list", json!({})).await;
    assert_eq!(schema["nodes"], json!([]));
    assert_eq!(schema["edges"], json!([]));

    client.cancel().await.unwrap();
    server.kill().await.expect("stop HTTP MCP server");
}

#[tokio::test]
async fn streamable_http_preserves_mcp_safety_annotations() {
    let (client, mut server) = http_client().await;
    let tools = client
        .peer()
        .list_tools(Default::default())
        .await
        .expect("list tools over Streamable HTTP");

    assert_read_only_tool_annotations(&tools);

    client.cancel().await.unwrap();
    server.kill().await.expect("stop HTTP MCP server");
}

#[tokio::test]
async fn grpc_service_mode_exercises_workspace_crud_and_reopen() {
    let temp = tempdir().unwrap();
    let (endpoint, service) = grpc_service(temp.path().to_path_buf()).await;
    let workspace_ref = unique_smoke_id();
    let client = grpc_mcp_client(&endpoint, &workspace_ref, "create").await;

    let schema = call(&client, "grm_schema_list", json!({})).await;
    assert_eq!(schema["backend"]["mode"], json!("grpc"));
    assert_eq!(schema["backend"]["workspace_format"], json!("binary"));
    assert_eq!(
        schema["backend"]["workspace_scope"],
        json!("ExecuteWorkspace")
    );
    assert_eq!(schema["nodes"], json!([]));

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
                        "name": "GrpcMcpUser",
                        "id_field": "userId",
                        "fields": [
                            { "name": "name", "type": "string", "required": true },
                            { "name": "smoke_id", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "GrpcMcpPost",
                        "id_field": "postId",
                        "fields": [
                            { "name": "title", "type": "string", "required": true },
                            { "name": "smoke_id", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "schema_define_edge",
                    "args": {
                        "name": "GRPC_MCP_AUTHORED",
                        "from_model": "GrpcMcpUser",
                        "to_model": "GrpcMcpPost",
                        "id_field": "authoredId",
                        "fields": [
                            { "name": "smoke_id", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "user",
                        "model": "GrpcMcpUser",
                        "props": { "name": "Alice", "smoke_id": workspace_ref }
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "post",
                        "model": "GrpcMcpPost",
                        "props": { "title": "MCP over gRPC", "smoke_id": workspace_ref }
                    }
                },
                {
                    "op": "edge_create",
                    "args": {
                        "model": "GRPC_MCP_AUTHORED",
                        "from": "user",
                        "to": "post",
                        "props": { "smoke_id": workspace_ref }
                    }
                }
            ]
        }),
    )
    .await;
    assert_eq!(result["applied"], json!(true));
    assert_eq!(result["counts"]["node_create"]["GrpcMcpUser"], json!(1));
    assert_eq!(
        result["counts"]["edge_create"]["GRPC_MCP_AUTHORED"],
        json!(1)
    );
    assert!(temp.path().join(format!("{workspace_ref}.bin")).exists());
    assert!(!temp.path().join(format!("{workspace_ref}.json")).exists());
    let user_id = result["ids"][0]["id"].as_i64().unwrap();
    let post_id = result["ids"][1]["id"].as_i64().unwrap();
    let edge_id = result["ids"][2]["id"].as_i64().unwrap();

    let updated_user = call(
        &client,
        "grm_node_update",
        json!({
            "model": "GrpcMcpUser",
            "id": user_id,
            "props": { "name": "Alice Updated" }
        }),
    )
    .await;
    assert_eq!(updated_user["props"]["name"], json!("Alice Updated"));

    let found_nodes = call(
        &client,
        "grm_node_find",
        json!({
            "model": "GrpcMcpUser",
            "filters": { "id": user_id, "name": "Alice Updated" }
        }),
    )
    .await;
    assert_eq!(found_nodes["nodes"].as_array().unwrap().len(), 1);

    let traversed = call(
        &client,
        "grm_node_find",
        json!({
            "model": "GrpcMcpUser",
            "filters": { "name": "Alice Updated" },
            "via": ["out:GRPC_MCP_AUTHORED:GrpcMcpPost"],
            "end_filters": { "title": "MCP over gRPC" },
            "edge_filters": { "smoke_id": workspace_ref },
            "return": "end",
            "order": "title:asc",
            "limit": 1,
            "offset": 0
        }),
    )
    .await;
    assert_eq!(traversed["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(traversed["nodes"][0]["id"], json!(post_id));

    let explain = call(
        &client,
        "grm_explain",
        json!({
            "command": "session.explain node.find GrpcMcpUser name=\"Alice Updated\" via=out:GRPC_MCP_AUTHORED:GrpcMcpPost end.title=\"MCP over gRPC\""
        }),
    )
    .await;
    assert_eq!(explain["command"], json!("node.find"));
    assert_eq!(explain["target"], json!("GrpcMcpUser"));
    assert!(
        explain["plan"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().is_some_and(|step| step.contains("ExpandOut")))
    );

    let profile = call(
        &client,
        "grm_profile",
        json!({
            "command": "session.profile node.find GrpcMcpUser name=\"Alice Updated\" via=out:GRPC_MCP_AUTHORED:GrpcMcpPost end.title=\"MCP over gRPC\""
        }),
    )
    .await;
    assert_eq!(profile["command"], json!("node.find"));
    assert_eq!(profile["target"], json!("GrpcMcpUser"));
    assert_eq!(profile["result_rows"], json!(1));
    assert!(profile["elapsed"]["micros"].as_u64().is_some());

    let edge_return = call(
        &client,
        "grm_node_find",
        json!({
            "model": "GrpcMcpUser",
            "filters": { "name": "Alice Updated" },
            "via": ["out:GRPC_MCP_AUTHORED:GrpcMcpPost"],
            "return": "edge"
        }),
    )
    .await;
    assert_eq!(edge_return["nodes"].as_array().unwrap().len(), 0);
    assert_eq!(edge_return["edges"].as_array().unwrap().len(), 1);
    assert_eq!(
        edge_return["edges"][0]["rel_type"],
        json!("GRPC_MCP_AUTHORED")
    );
    assert_eq!(edge_return["edges"][0]["from"], json!(user_id));
    assert_eq!(edge_return["edges"][0]["to"], json!(post_id));

    let found_edges = call(
        &client,
        "grm_edge_find",
        json!({
            "model": "GRPC_MCP_AUTHORED",
            "filters": { "id": edge_id, "from": user_id, "to": post_id }
        }),
    )
    .await;
    assert_eq!(found_edges["edges"].as_array().unwrap().len(), 1);

    let edge_profile = call(
        &client,
        "grm_profile",
        json!({
            "command": format!("session.profile edge.find GRPC_MCP_AUTHORED from={user_id}")
        }),
    )
    .await;
    assert_eq!(edge_profile["command"], json!("edge.find"));
    assert_eq!(edge_profile["target"], json!("GRPC_MCP_AUTHORED"));
    assert_eq!(edge_profile["result_rows"], json!(1));
    assert!(
        edge_profile["plan"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step
                .as_str()
                .is_some_and(|step| step.contains("RelationshipEndpointSeek")))
    );

    let unsupported_query = call_error(
        &client,
        "grm_query",
        json!({ "command": "node.find GrpcMcpUser name=\"Alice Updated\"" }),
    )
    .await;
    assert!(unsupported_query.contains("gRPC MCP mode"));

    client.cancel().await.unwrap();

    let reopened = grpc_mcp_client(&endpoint, &workspace_ref, "open").await;
    let reopened_schema = call(&reopened, "grm_schema_list", json!({})).await;
    assert_eq!(reopened_schema["backend"]["mode"], json!("grpc"));
    assert_eq!(
        reopened_schema["backend"]["workspace_format"],
        json!("binary")
    );
    assert!(
        reopened_schema["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["name"] == json!("GrpcMcpUser"))
    );
    let reopened_nodes = call(
        &reopened,
        "grm_node_find",
        json!({
            "model": "GrpcMcpUser",
            "filters": { "id": user_id, "name": "Alice Updated" }
        }),
    )
    .await;
    assert_eq!(reopened_nodes["nodes"].as_array().unwrap().len(), 1);

    reopened.cancel().await.unwrap();
    service.abort();
}

#[tokio::test]
async fn grpc_service_mode_accepts_explicit_json_workspace_format() {
    let temp = tempdir().unwrap();
    let (endpoint, service) = grpc_service(temp.path().to_path_buf()).await;
    let workspace_ref = unique_smoke_id();
    let client =
        grpc_mcp_client_with_format(&endpoint, &workspace_ref, "create", Some("json")).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "GrpcMcpJsonUser",
            "id_field": "userId",
            "fields": [
                { "name": "name", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    let schema = call(&client, "grm_schema_list", json!({})).await;
    assert_eq!(schema["backend"]["workspace_format"], json!("json"));
    assert!(temp.path().join(format!("{workspace_ref}.json")).exists());
    assert!(!temp.path().join(format!("{workspace_ref}.bin")).exists());

    client.cancel().await.unwrap();
    service.abort();
}

#[tokio::test]
async fn grpc_service_mode_create_or_open_reuses_existing_workspace() {
    let temp = tempdir().unwrap();
    let (endpoint, service) = grpc_service(temp.path().to_path_buf()).await;
    let workspace_ref = unique_smoke_id();
    let creator = grpc_mcp_client(&endpoint, &workspace_ref, "create").await;

    call(
        &creator,
        "grm_schema_define_node",
        json!({
            "name": "GrpcMcpReusableUser",
            "id_field": "userId",
            "fields": [
                { "name": "name", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    creator.cancel().await.unwrap();

    let reused = grpc_mcp_client(&endpoint, &workspace_ref, "create-or-open").await;
    let schema = call(&reused, "grm_schema_list", json!({})).await;
    assert!(
        schema["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["name"] == json!("GrpcMcpReusableUser"))
    );

    reused.cancel().await.unwrap();
    service.abort();
}

#[tokio::test]
#[ignore = "requires a running Neo4j Bolt endpoint and NEO4J_* env vars"]
async fn neo4j_batch_defines_schema_creates_graph_and_finds_records() {
    let Some(client) = neo4j_client().await else {
        eprintln!("skipping Neo4j MCP smoke test; set NEO4J_URI, NEO4J_USER, and NEO4J_PASSWORD");
        return;
    };
    let smoke_id = unique_smoke_id();

    let schema = call(&client, "grm_schema_list", json!({})).await;
    assert_eq!(schema["backend"]["mode"], json!("neo4j"));
    assert_eq!(schema["backend"]["runtime_schema_empty"], json!(true));
    assert!(
        schema
            .to_string()
            .contains("Define or reconstruct session-local runtime schema")
    );

    let missing_schema = call_error(
        &client,
        "grm_node_find",
        json!({ "model": "GrmMcpSmokeUser", "filters": { "smoke_id": smoke_id } }),
    )
    .await;
    assert!(missing_schema.contains("session-local runtime schema"));
    assert!(missing_schema.contains("define schema first"));
    assert!(missing_schema.contains("grm_schema_list"));

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
                        "name": "GrmMcpSmokeUser",
                        "id_field": "userId",
                        "fields": [
                            { "name": "name", "type": "string", "required": true },
                            { "name": "smoke_id", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "GrmMcpSmokePost",
                        "id_field": "postId",
                        "fields": [
                            { "name": "title", "type": "string", "required": true },
                            { "name": "smoke_id", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "schema_define_edge",
                    "args": {
                        "name": "GRM_MCP_SMOKE_AUTHORED",
                        "from_model": "GrmMcpSmokeUser",
                        "to_model": "GrmMcpSmokePost",
                        "id_field": "authoredId",
                        "fields": [
                            { "name": "smoke_id", "type": "string", "required": true }
                        ]
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "user",
                        "model": "GrmMcpSmokeUser",
                        "props": { "name": "Alice", "smoke_id": smoke_id }
                    }
                },
                {
                    "op": "node_create",
                    "args": {
                        "ref": "post",
                        "model": "GrmMcpSmokePost",
                        "props": { "title": "Neo4j MCP batch smoke", "smoke_id": smoke_id }
                    }
                },
                {
                    "op": "edge_create",
                    "args": {
                        "model": "GRM_MCP_SMOKE_AUTHORED",
                        "from": "user",
                        "to": "post",
                        "props": { "smoke_id": smoke_id }
                    }
                }
            ]
        }),
    )
    .await;

    assert_eq!(result["applied"], json!(true));
    assert_eq!(result["backend"]["mode"], json!("neo4j"));
    assert!(
        result["backend"]["atomicity"]
            .as_str()
            .unwrap()
            .contains("one transaction")
    );
    assert_eq!(result["counts"]["node_create"]["GrmMcpSmokeUser"], json!(1));
    assert_eq!(
        result["counts"]["edge_create"]["GRM_MCP_SMOKE_AUTHORED"],
        json!(1)
    );
    assert_eq!(result["ids"].as_array().unwrap().len(), 3);
    let user_id = result["ids"][0]["id"].as_i64().unwrap();
    let post_id = result["ids"][1]["id"].as_i64().unwrap();
    let edge_id = result["ids"][2]["id"].as_i64().unwrap();

    let updated_user = call(
        &client,
        "grm_node_update",
        json!({
            "model": "GrmMcpSmokeUser",
            "id": user_id,
            "props": { "name": "Alice Updated" }
        }),
    )
    .await;
    assert_eq!(updated_user["props"]["name"], json!("Alice Updated"));

    let found_nodes = call(
        &client,
        "grm_node_find",
        json!({ "model": "GrmMcpSmokeUser", "filters": { "id": user_id, "name": "Alice Updated" } }),
    )
    .await;
    assert_eq!(found_nodes["nodes"].as_array().unwrap().len(), 1);

    call(
        &client,
        "grm_node_create",
        json!({
            "model": "GrmMcpSmokeUser",
            "props": { "name": "Zoe", "smoke_id": smoke_id }
        }),
    )
    .await;
    let paged_nodes = call(
        &client,
        "grm_node_find",
        json!({
            "model": "GrmMcpSmokeUser",
            "filters": { "smoke_id": smoke_id, "order": "userId:asc", "limit": 1 }
        }),
    )
    .await;
    assert_eq!(paged_nodes["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(
        paged_nodes["nodes"][0]["props"]["name"],
        json!("Alice Updated")
    );

    let updated_edge = call(
        &client,
        "grm_edge_update",
        json!({
            "model": "GRM_MCP_SMOKE_AUTHORED",
            "id": edge_id,
            "props": { "smoke_id": smoke_id }
        }),
    )
    .await;
    assert_eq!(updated_edge["id"], json!(edge_id));

    let found_edges = call(
        &client,
        "grm_edge_find",
        json!({ "model": "GRM_MCP_SMOKE_AUTHORED", "filters": { "id": edge_id, "from": user_id, "to": post_id } }),
    )
    .await;
    assert_eq!(found_edges["edges"].as_array().unwrap().len(), 1);

    let summary_resource = client
        .read_resource(ReadResourceRequestParams::new("grm://graph/summary"))
        .await
        .expect("read Neo4j graph summary resource");
    let [ResourceContents::TextResourceContents { text, .. }] =
        summary_resource.contents.as_slice()
    else {
        panic!("expected one text graph summary resource");
    };
    let summary: Value = serde_json::from_str(text).expect("parse Neo4j summary resource JSON");
    assert_eq!(summary["backend"]["mode"], json!("neo4j"));
    assert_eq!(
        summary["backend"]["scope"],
        json!("session-local runtime schema models")
    );
    assert_eq!(summary["nodes"]["by_model"]["GrmMcpSmokeUser"], json!(2));
    assert_eq!(summary["nodes"]["by_model"]["GrmMcpSmokePost"], json!(1));
    assert_eq!(
        summary["edges"]["by_model"]["GRM_MCP_SMOKE_AUTHORED"],
        json!(1)
    );

    let paged_edges = call(
        &client,
        "grm_edge_find",
        json!({
            "model": "GRM_MCP_SMOKE_AUTHORED",
            "filters": { "smoke_id": smoke_id, "order": "from:asc,to:asc", "limit": 1 }
        }),
    )
    .await;
    assert_eq!(paged_edges["edges"].as_array().unwrap().len(), 1);

    let delete_rejected = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "ops": [
                {
                    "op": "edge_delete",
                    "args": { "model": "GRM_MCP_SMOKE_AUTHORED", "id": edge_id }
                }
            ]
        }),
    )
    .await;
    assert_eq!(delete_rejected["applied"], json!(false));
    assert!(
        delete_rejected["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("requires allow_deletes=true")
    );

    let mutation_result = call(
        &client,
        "grm_batch",
        json!({
            "atomic": true,
            "allow_deletes": true,
            "response": "detailed",
            "ops": [
                {
                    "op": "node_update",
                    "args": {
                        "model": "GrmMcpSmokePost",
                        "id": post_id,
                        "props": { "title": "Neo4j MCP batch smoke updated" }
                    }
                },
                {
                    "op": "edge_update",
                    "args": {
                        "model": "GRM_MCP_SMOKE_AUTHORED",
                        "id": edge_id,
                        "props": { "smoke_id": smoke_id }
                    }
                },
                {
                    "op": "edge_delete",
                    "args": { "model": "GRM_MCP_SMOKE_AUTHORED", "id": edge_id }
                },
                {
                    "op": "node_delete",
                    "args": { "model": "GrmMcpSmokePost", "id": post_id }
                }
            ]
        }),
    )
    .await;
    assert_eq!(mutation_result["applied"], json!(true));
    assert_eq!(
        mutation_result["counts"]["node_update"]["GrmMcpSmokePost"],
        json!(1)
    );
    assert_eq!(
        mutation_result["counts"]["edge_update"]["GRM_MCP_SMOKE_AUTHORED"],
        json!(1)
    );
    assert_eq!(
        mutation_result["counts"]["edge_delete"]["GRM_MCP_SMOKE_AUTHORED"],
        json!(1)
    );
    assert_eq!(
        mutation_result["counts"]["node_delete"]["GrmMcpSmokePost"],
        json!(1)
    );

    let deleted_edge = call(
        &client,
        "grm_edge_find",
        json!({ "model": "GRM_MCP_SMOKE_AUTHORED", "filters": { "id": edge_id } }),
    )
    .await;
    assert_eq!(deleted_edge["edges"].as_array().unwrap().len(), 0);

    let deleted_post = call(
        &client,
        "grm_node_find",
        json!({ "model": "GrmMcpSmokePost", "filters": { "id": post_id } }),
    )
    .await;
    assert_eq!(deleted_post["nodes"].as_array().unwrap().len(), 0);

    let delete_target_post = call(
        &client,
        "grm_node_create",
        json!({
            "model": "GrmMcpSmokePost",
            "props": { "title": "Neo4j MCP single delete target", "smoke_id": smoke_id }
        }),
    )
    .await;
    let delete_target_post_id = delete_target_post["id"].as_i64().unwrap();
    let delete_target_edge = call(
        &client,
        "grm_edge_create",
        json!({
            "model": "GRM_MCP_SMOKE_AUTHORED",
            "from": user_id,
            "to": delete_target_post_id,
            "props": { "smoke_id": smoke_id }
        }),
    )
    .await;
    let delete_target_edge_id = delete_target_edge["id"].as_i64().unwrap();

    let edge_delete = call(
        &client,
        "grm_edge_delete",
        json!({ "model": "GRM_MCP_SMOKE_AUTHORED", "id": delete_target_edge_id }),
    )
    .await;
    assert_eq!(edge_delete["deleted"], json!(true));

    let deleted_single_edge = call(
        &client,
        "grm_edge_find",
        json!({ "model": "GRM_MCP_SMOKE_AUTHORED", "filters": { "id": delete_target_edge_id } }),
    )
    .await;
    assert_eq!(deleted_single_edge["edges"].as_array().unwrap().len(), 0);

    call(
        &client,
        "grm_node_delete",
        json!({ "model": "GrmMcpSmokePost", "id": delete_target_post_id }),
    )
    .await;

    let user_delete = call(
        &client,
        "grm_node_delete",
        json!({ "model": "GrmMcpSmokeUser", "id": user_id }),
    )
    .await;
    assert_eq!(user_delete["deleted"], json!(true));

    let deleted_user = call(
        &client,
        "grm_node_find",
        json!({ "model": "GrmMcpSmokeUser", "filters": { "id": user_id } }),
    )
    .await;
    assert_eq!(deleted_user["nodes"].as_array().unwrap().len(), 0);

    client.cancel().await.unwrap();
    cleanup_neo4j_smoke_graph(&smoke_id).await;
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
async fn read_only_tools_expose_mcp_safety_annotations() {
    let client = client(&[]).await;
    let tools = client.list_tools(None).await.expect("list tools");

    assert_read_only_tool_annotations(&tools);

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn schema_checkpoint_is_exposed_and_rejects_in_memory_mode() {
    let client = client(&[]).await;
    let tools = client.list_tools(None).await.expect("list tools");
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "grm_schema_checkpoint")
    );

    let error = call_error(&client, "grm_schema_checkpoint", json!({})).await;
    assert!(error.contains("only supported in Neo4j MCP mode"));

    let help = call(
        &client,
        "grm_tool_help",
        json!({ "tool": "grm_schema_checkpoint" }),
    )
    .await;
    assert!(help.to_string().contains(
        "does not create, update, delete, compact, or otherwise modify Neo4j graph data"
    ));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn find_tools_accept_adapter_filters_through_public_mcp_surface() {
    let client = client(&[]).await;

    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "User",
            "id_field": "userId",
            "fields": [
                { "name": "name", "type": "string", "required": true },
                { "name": "age", "type": "int", "required": true }
            ]
        }),
    )
    .await;
    call(
        &client,
        "grm_schema_define_node",
        json!({
            "name": "Post",
            "id_field": "postId",
            "fields": [
                { "name": "title", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    call(
        &client,
        "grm_schema_define_edge",
        json!({
            "name": "Authored",
            "from_model": "User",
            "to_model": "Post",
            "id_field": "authoredId",
            "fields": [
                { "name": "year", "type": "int", "required": true }
            ]
        }),
    )
    .await;

    call(
        &client,
        "grm_node_create",
        json!({ "model": "User", "props": { "name": "Alice", "age": 42 } }),
    )
    .await;
    let bob = call(
        &client,
        "grm_node_create",
        json!({ "model": "User", "props": { "name": "Bob", "age": 37 } }),
    )
    .await;
    let post = call(
        &client,
        "grm_node_create",
        json!({ "model": "Post", "props": { "title": "Hello" } }),
    )
    .await;
    call(
        &client,
        "grm_edge_create",
        json!({
            "model": "Authored",
            "from": bob["id"],
            "to": post["id"],
            "props": { "year": 2026 }
        }),
    )
    .await;

    let found_nodes = call(
        &client,
        "grm_node_find",
        json!({
            "model": "User",
            "filters": { "age>": 35, "order": "age:asc", "limit": 1 }
        }),
    )
    .await;
    assert_eq!(found_nodes["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(found_nodes["nodes"][0]["props"]["name"], json!("Bob"));

    let found_edges = call(
        &client,
        "grm_edge_find",
        json!({
            "model": "Authored",
            "filters": { "from": bob["id"], "year": 2026 }
        }),
    )
    .await;
    assert_eq!(found_edges["edges"].as_array().unwrap().len(), 1);
    assert_eq!(found_edges["edges"][0]["to"], post["id"]);

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
    let writer = client(&["--autocommit-json", &path_arg]).await;

    call(
        &writer,
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

    writer.cancel().await.unwrap();

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

    let reopened = client(&["--autocommit-json", &path_arg]).await;
    let found = call(
        &reopened,
        "grm_node_find",
        json!({ "model": "User", "filters": { "name": "Alice" } }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);
    reopened.cancel().await.unwrap();
}

#[tokio::test]
async fn autocommit_single_operation_tools_append_wal_records() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("mcp-single-session.json");
    let path_arg = path.to_string_lossy().into_owned();
    let writer = client(&["--autocommit-json", &path_arg]).await;

    call(
        &writer,
        "grm_schema_define_node",
        json!({
            "name": "User",
            "id_field": "userId",
            "fields": [
                { "name": "name", "type": "string", "required": true }
            ]
        }),
    )
    .await;
    call(
        &writer,
        "grm_node_create",
        json!({
            "model": "User",
            "props": { "name": "Alice" }
        }),
    )
    .await;

    writer.cancel().await.unwrap();

    let log = std::fs::read_to_string(path.with_extension("json.log")).unwrap();
    assert!(log.contains("RegisterNodeModel"));
    assert!(log.contains("UpsertNode"));

    let reopened = client(&["--autocommit-json", &path_arg]).await;
    let found = call(
        &reopened,
        "grm_node_find",
        json!({ "model": "User", "filters": { "name": "Alice" } }),
    )
    .await;
    assert_eq!(found["nodes"].as_array().unwrap().len(), 1);
    reopened.cancel().await.unwrap();
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
