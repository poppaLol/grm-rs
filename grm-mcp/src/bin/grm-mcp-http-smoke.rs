use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::{Value, json};

const SMOKE_MODEL: &str = "McpHttpSmokeUser";

#[tokio::main]
async fn main() {
    let mut expect_schema_list_permission_denied = false;
    let mut endpoint = None;
    for arg in std::env::args().skip(1) {
        if arg == "--expect-schema-list-permission-denied" {
            expect_schema_list_permission_denied = true;
        } else {
            endpoint = Some(arg);
        }
    }
    let endpoint = endpoint.unwrap_or_else(|| {
        std::env::var("GRM_MCP_HTTP_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:8080/mcp".into())
    });

    let result = if expect_schema_list_permission_denied {
        run_schema_list_permission_denied(&endpoint).await
    } else {
        run(&endpoint).await
    };
    if let Err(err) = result {
        eprintln!("grm-mcp HTTP smoke failed: {err}");
        std::process::exit(1);
    }
}

async fn run(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = ().serve(StreamableHttpClientTransport::from_uri(endpoint)).await?;
    let tools = client.peer().list_tools(Default::default()).await?;
    if !tools
        .tools
        .iter()
        .any(|tool| tool.name.as_ref() == "grm_schema_list")
    {
        return Err("grm_schema_list was not advertised by tools/list".into());
    }

    let structured = call(&client, "grm_schema_list", json!({})).await?;
    if structured.get("nodes").is_none() || structured.get("edges").is_none() {
        return Err(format!("unexpected grm_schema_list response: {structured}").into());
    }

    ensure_smoke_model(&client, &structured).await?;
    call(
        &client,
        "grm_node_create",
        json!({
            "model": SMOKE_MODEL,
            "props": { "name": "Ada" }
        }),
    )
    .await?;
    let found = call(
        &client,
        "grm_node_find",
        json!({
            "model": SMOKE_MODEL,
            "filters": { "name": "Ada" }
        }),
    )
    .await?;
    if found
        .get("nodes")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(format!("grm_node_find did not return the smoke node: {found}").into());
    }

    let command = format!("node.find {SMOKE_MODEL} name=\"Ada\"");
    let explain = call(&client, "grm_explain", json!({ "command": command })).await?;
    if explain.get("command") != Some(&json!("node.find")) {
        return Err(format!("unexpected grm_explain response: {explain}").into());
    }

    let command = format!("node.find {SMOKE_MODEL} name=\"Ada\"");
    let profile = call(&client, "grm_profile", json!({ "command": command })).await?;
    let result_rows = profile
        .get("result_rows")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("unexpected grm_profile response: {profile}"))?;
    if result_rows == 0 {
        return Err(format!("unexpected grm_profile response: {profile}").into());
    }

    let backend = structured
        .get("backend")
        .and_then(|backend| backend.get("mode"))
        .cloned()
        .unwrap_or_else(|| json!("in-memory"));
    println!(
        "ok: Streamable HTTP MCP initialized, listed tools, and exercised schema/find/explain/profile through backend {backend}"
    );
    client.cancel().await?;
    Ok(())
}

async fn run_schema_list_permission_denied(
    endpoint: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = ().serve(StreamableHttpClientTransport::from_uri(endpoint)).await?;
    let tools = client.peer().list_tools(Default::default()).await?;
    if !tools
        .tools
        .iter()
        .any(|tool| tool.name.as_ref() == "grm_schema_list")
    {
        return Err("grm_schema_list was not advertised by tools/list".into());
    }

    let err = raw_call(&client, "grm_schema_list", json!({}))
        .await
        .expect_err("grm_schema_list should fail for the limited principal");
    let message = err.to_string();
    if !message.contains("PermissionDenied") {
        return Err(format!(
            "expected PermissionDenied category from grm_schema_list, got: {message}"
        )
        .into());
    }
    if !message.contains("authorization denied") {
        return Err(format!(
            "expected closed authorization denial from grm_schema_list, got: {message}"
        )
        .into());
    }
    if !message.contains("grm_schema_list") && !message.contains("schema.inspect") {
        return Err(format!(
            "expected grm_schema_list/schema.inspect denied shape, got: {message}"
        )
        .into());
    }

    println!(
        "ok: Streamable HTTP MCP initialized, listed tools, and grm_schema_list failed permission denied"
    );
    client.cancel().await?;
    Ok(())
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let result = raw_call(client, name, args).await?;
    result
        .structured_content
        .ok_or_else(|| format!("{name} did not return structured content").into())
}

async fn raw_call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: Value,
) -> Result<rmcp::model::CallToolResult, Box<dyn std::error::Error>> {
    let arguments: JsonObject = args.as_object().cloned().unwrap_or_default();
    client
        .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(arguments))
        .await
        .map_err(|err| format!("{name} failed: {err}").into())
}

async fn ensure_smoke_model(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    schema: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if schema_has_node_model(schema, SMOKE_MODEL) {
        return Ok(());
    }
    call(
        client,
        "grm_schema_define_node",
        json!({
            "name": SMOKE_MODEL,
            "id_field": "userId",
            "fields": [
                { "name": "name", "type": "string", "required": true }
            ]
        }),
    )
    .await?;
    Ok(())
}

fn schema_has_node_model(schema: &Value, model: &str) -> bool {
    schema
        .get("nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|node| node.get("name").and_then(Value::as_str) == Some(model))
}
