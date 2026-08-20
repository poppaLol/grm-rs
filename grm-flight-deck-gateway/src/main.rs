use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    future::Future,
    net::SocketAddr,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
    routing::{get, post},
};
use grm_rs::{
    EdgeFindRequest, ExplainRequest, NodeFindRequest, ProfileRequest, QueryRequest, QueryTerm,
    RuntimeField, RuntimeValueType, StoredNode, StoredRel,
    runtime::{SessionCommand, parse_command_line},
};
use grm_service_api::{
    DurabilityFormat, GrpcClientTlsOptions, GrpcWorkspaceClient, GrpcWorkspaceClientError,
    GrpcWorkspaceMode, grpc_security_status, proto,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::cors::{AllowOrigin, CorsLayer};

const DEFAULT_MODEL_LIMIT: usize = 200;
const DEFAULT_FLIGHT_DECK_ALLOWED_ORIGINS: &str =
    "http://127.0.0.1:8081,http://localhost:8081,http://127.0.0.1:3001,http://localhost:3001";

#[derive(Clone)]
struct AppState {
    grpc_endpoint: Arc<str>,
}

#[derive(Deserialize)]
struct SnapshotQuery {
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSnapshot {
    workspace: String,
    node_models: Vec<String>,
    edge_models: Vec<String>,
    schema_node_models: Vec<FlightDeckSchemaNodeModel>,
    schema_edge_models: Vec<FlightDeckSchemaEdgeModel>,
    schema_edges: Vec<FlightDeckSchemaEdge>,
    nodes: Vec<FlightDeckNode>,
    edges: Vec<FlightDeckEdge>,
    model_limit: usize,
    omitted_edges: usize,
}

#[derive(Debug, Clone, Serialize)]
struct FlightDeckNode {
    id: String,
    model: String,
    label: String,
    props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct FlightDeckEdge {
    id: String,
    model: String,
    from: String,
    to: String,
    props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSchemaEdge {
    model: String,
    from_model: String,
    to_model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSchemaField {
    name: String,
    value_type: String,
    required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSchemaNodeModel {
    name: String,
    id_field: String,
    fields: Vec<FlightDeckSchemaField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSchemaEdgeModel {
    name: String,
    from_model: String,
    to_model: String,
    id_field: String,
    fields: Vec<FlightDeckSchemaField>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckQueryRequest {
    command: String,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckQueryResponse {
    workspace: String,
    command: String,
    kind: FlightDeckQueryKind,
    query_shape: String,
    result: FlightDeckSnapshot,
    evidence: FlightDeckQueryEvidence,
    partial_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FlightDeckQueryKind {
    Query,
    Explain,
    Profile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckQueryEvidence {
    provenance: String,
    label: String,
    plan_kind: Option<String>,
    steps: Vec<String>,
    indexes: Vec<String>,
    row_count: Option<u64>,
    elapsed_micros: Option<u64>,
    unsupported_reason: Option<String>,
}

#[derive(Debug, Clone)]
enum FlightDeckQueryPlan {
    NodeFind(NodeFindRequest),
    EdgeFind(EdgeFindRequest),
}

#[derive(Debug, Clone)]
struct ParsedFlightDeckQuery {
    command: String,
    kind: FlightDeckQueryKind,
    plan: FlightDeckQueryPlan,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSecurityStatus {
    security_profile: String,
    identity_status: String,
    principal: Option<FlightDeckPrincipal>,
    authentication_method: Option<String>,
    policy_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FlightDeckPrincipal {
    issuer: String,
    subject: String,
}

async fn snapshot(
    State(state): State<AppState>,
    Path(workspace): Path<String>,
    Query(query): Query<SnapshotQuery>,
) -> Result<Json<FlightDeckSnapshot>, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(DEFAULT_MODEL_LIMIT).clamp(1, 1_000);
    load_snapshot(state.grpc_endpoint.to_string(), workspace, limit)
        .await
        .map(Json)
        .map_err(|error| redacted_gateway_error("snapshot", &error))
}

async fn security_status(
    State(state): State<AppState>,
) -> Result<Json<FlightDeckSecurityStatus>, (StatusCode, String)> {
    load_security_status(state.grpc_endpoint.to_string())
        .await
        .map(Json)
        .map_err(|error| redacted_gateway_error("security status", &error))
}

async fn query_command(
    State(state): State<AppState>,
    Path(workspace): Path<String>,
    Json(request): Json<FlightDeckQueryRequest>,
) -> Result<Json<FlightDeckQueryResponse>, (StatusCode, String)> {
    let limit = request.limit.unwrap_or(DEFAULT_MODEL_LIMIT).clamp(1, 1_000);
    let parsed = parse_read_only_query_command(&request.command, limit)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;

    load_query_command(state.grpc_endpoint.to_string(), workspace, parsed)
        .await
        .map(Json)
        .map_err(|error| redacted_gateway_error("query", &error))
}

fn redacted_gateway_error(
    operation: &'static str,
    _error: &dyn std::error::Error,
) -> (StatusCode, String) {
    eprintln!("{}", redacted_gateway_log_message(operation));
    (
        StatusCode::BAD_GATEWAY,
        format!("failed to load {operation} from GRM service"),
    )
}

fn redacted_gateway_log_message(operation: &'static str) -> String {
    format!("flight-deck gateway {operation} error: upstream request failed")
}

async fn load_snapshot(
    endpoint: impl Into<String>,
    workspace: impl Into<String>,
    model_limit: usize,
) -> Result<FlightDeckSnapshot, SnapshotLoadError> {
    let endpoint = endpoint.into();
    let workspace = workspace.into();
    let tls = GrpcClientTlsOptions::from_env().map_err(GrpcWorkspaceClientError::TlsConfig)?;
    let mut client = GrpcWorkspaceClient::connect_with_format_and_tls(
        endpoint,
        workspace.clone(),
        GrpcWorkspaceMode::Open,
        DurabilityFormat::Binary,
        tls,
    )
    .await?;

    let snapshot_result = collect_snapshot(&mut client, workspace, model_limit).await;
    close_after_snapshot(snapshot_result, async { client.close().await.map(|_| ()) }).await
}

async fn load_security_status(
    endpoint: impl Into<String>,
) -> Result<FlightDeckSecurityStatus, GrpcWorkspaceClientError> {
    let tls = GrpcClientTlsOptions::from_env().map_err(GrpcWorkspaceClientError::TlsConfig)?;
    let status = grpc_security_status(endpoint, tls).await?;
    Ok(flight_deck_security_status(status))
}

async fn load_query_command(
    endpoint: impl Into<String>,
    workspace: impl Into<String>,
    parsed: ParsedFlightDeckQuery,
) -> Result<FlightDeckQueryResponse, QueryLoadError> {
    let endpoint = endpoint.into();
    let workspace = workspace.into();
    let tls = GrpcClientTlsOptions::from_env().map_err(GrpcWorkspaceClientError::TlsConfig)?;
    let mut client = GrpcWorkspaceClient::connect_with_format_and_tls(
        endpoint,
        workspace.clone(),
        GrpcWorkspaceMode::Open,
        DurabilityFormat::Binary,
        tls,
    )
    .await?;

    let result = collect_query_result(&mut client, workspace, parsed).await;
    match result {
        Ok(response) => {
            client.close().await.map_err(QueryLoadError::Close)?;
            Ok(response)
        }
        Err(primary) => {
            let close_error = client.close().await.err().map(|error| error.to_string());
            Err(QueryLoadError::Query {
                primary,
                close_error,
            })
        }
    }
}

async fn collect_query_result(
    client: &mut GrpcWorkspaceClient,
    workspace: String,
    parsed: ParsedFlightDeckQuery,
) -> Result<FlightDeckQueryResponse, GrpcWorkspaceClientError> {
    let schema = client.schema_list().await?;
    let schema_node_models = schema
        .node_models
        .iter()
        .map(|model| FlightDeckSchemaNodeModel {
            name: model.name.clone(),
            id_field: model.id_field_name.clone(),
            fields: schema_fields(&model.fields),
        })
        .collect::<Vec<_>>();
    let schema_edge_models = schema
        .edge_models
        .iter()
        .map(|model| FlightDeckSchemaEdgeModel {
            name: model.name.clone(),
            from_model: model.from_model.clone(),
            to_model: model.to_model.clone(),
            id_field: model.id_field_name.clone(),
            fields: schema_fields(&model.fields),
        })
        .collect::<Vec<_>>();
    let node_models = schema_node_models
        .iter()
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    let edge_models = schema_edge_models
        .iter()
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    let schema_edges = schema_edge_models
        .iter()
        .map(|model| FlightDeckSchemaEdge {
            model: model.name.clone(),
            from_model: model.from_model.clone(),
            to_model: model.to_model.clone(),
        })
        .collect::<Vec<_>>();

    let (nodes, edges, omitted_edges, partial_reason) = match &parsed.plan {
        FlightDeckQueryPlan::NodeFind(request) => {
            let found = client.find_node_results(request.clone()).await?;
            let nodes = found
                .nodes
                .into_iter()
                .map(flight_deck_node)
                .collect::<Vec<_>>();
            let known_node_ids = nodes
                .iter()
                .map(|node| node.id.clone())
                .collect::<BTreeSet<_>>();
            let mut omitted_edges = 0;
            let edges = found
                .edges
                .into_iter()
                .filter_map(|edge| {
                    if known_node_ids.contains(&edge.from.to_string())
                        && known_node_ids.contains(&edge.to.to_string())
                    {
                        Some(flight_deck_edge(edge))
                    } else {
                        omitted_edges += 1;
                        None
                    }
                })
                .collect::<Vec<_>>();
            (nodes, edges, omitted_edges, None)
        }
        FlightDeckQueryPlan::EdgeFind(request) => {
            let found = client.find_edges(request.clone()).await?;
            let (nodes, edges, omitted_edges) =
                edge_result_with_known_endpoints(client, &schema_edge_models, request, found.edges)
                    .await?;
            let partial_reason = if omitted_edges > 0 {
                Some(
                    "some edge endpoints were unavailable from the bounded typed query result"
                        .into(),
                )
            } else {
                None
            };
            (nodes, edges, omitted_edges, partial_reason)
        }
    };

    let evidence = match parsed.kind {
        FlightDeckQueryKind::Query => FlightDeckQueryEvidence {
            provenance: "service".into(),
            label: "service-backed typed query result".into(),
            plan_kind: None,
            steps: Vec::new(),
            indexes: Vec::new(),
            row_count: None,
            elapsed_micros: None,
            unsupported_reason: None,
        },
        FlightDeckQueryKind::Explain => service_explain_evidence(client, &parsed.plan).await?,
        FlightDeckQueryKind::Profile => service_profile_evidence(client, &parsed.plan).await?,
    };

    let result = FlightDeckSnapshot {
        workspace: workspace.clone(),
        node_models,
        edge_models,
        schema_node_models,
        schema_edge_models,
        schema_edges,
        nodes,
        edges,
        model_limit: query_limit(&parsed.plan).unwrap_or(DEFAULT_MODEL_LIMIT),
        omitted_edges,
    };

    Ok(FlightDeckQueryResponse {
        workspace,
        command: parsed.command,
        kind: parsed.kind,
        query_shape: query_shape(&parsed.plan).into(),
        result,
        evidence,
        partial_reason,
    })
}

fn flight_deck_security_status(status: proto::SecurityStatusResponse) -> FlightDeckSecurityStatus {
    let authentication_method = non_empty(status.authentication_method);
    FlightDeckSecurityStatus {
        security_profile: security_profile_label(status.security_profile),
        identity_status: security_identity_status_label(status.identity_status),
        principal: status.principal.map(|principal| FlightDeckPrincipal {
            issuer: principal.issuer,
            subject: principal.subject,
        }),
        authentication_method,
        policy_version: non_empty(status.policy_version),
    }
}

fn security_profile_label(value: i32) -> String {
    match proto::SecurityProfile::try_from(value).ok() {
        Some(proto::SecurityProfile::AnonymousLocal) => "anonymous_local",
        Some(proto::SecurityProfile::DockerLocalInsecure) => "docker_local_insecure",
        Some(proto::SecurityProfile::Secured) => "secured",
        _ => "unknown",
    }
    .into()
}

fn security_identity_status_label(value: i32) -> String {
    match proto::SecurityIdentityStatus::try_from(value).ok() {
        Some(proto::SecurityIdentityStatus::AnonymousLocal) => "anonymous_local",
        Some(proto::SecurityIdentityStatus::DockerLocalInsecure) => "docker_local_insecure",
        Some(proto::SecurityIdentityStatus::AuthenticatedPrincipal) => "authenticated_principal",
        _ => "unknown",
    }
    .into()
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

fn parse_read_only_query_command(
    command: &str,
    default_limit: usize,
) -> Result<ParsedFlightDeckQuery, String> {
    reject_unsafe_command_text(command)?;
    let command = command.trim();
    if command.is_empty() {
        return Err("enter a read-only GRM command".into());
    }

    match parse_command_line(command).map_err(|error| error.to_string())? {
        SessionCommand::NodeFind { model_name, terms } => {
            let request = bounded_node_find_request(model_name, terms, default_limit)?;
            reject_edge_return(&request)?;
            Ok(ParsedFlightDeckQuery {
                command: command.into(),
                kind: FlightDeckQueryKind::Query,
                plan: FlightDeckQueryPlan::NodeFind(request),
            })
        }
        SessionCommand::EdgeFind { model_name, terms } => {
            let request = bounded_edge_find_request(model_name, terms, default_limit)?;
            Ok(ParsedFlightDeckQuery {
                command: command.into(),
                kind: FlightDeckQueryKind::Query,
                plan: FlightDeckQueryPlan::EdgeFind(request),
            })
        }
        SessionCommand::SessionExplainNodeFind {
            model_name, terms, ..
        } => {
            let request = bounded_node_find_request(model_name, terms, default_limit)?;
            reject_edge_return(&request)?;
            Ok(ParsedFlightDeckQuery {
                command: command.into(),
                kind: FlightDeckQueryKind::Explain,
                plan: FlightDeckQueryPlan::NodeFind(request),
            })
        }
        SessionCommand::SessionExplainEdgeFind {
            model_name, terms, ..
        } => {
            let request = bounded_edge_find_request(model_name, terms, default_limit)?;
            Ok(ParsedFlightDeckQuery {
                command: command.into(),
                kind: FlightDeckQueryKind::Explain,
                plan: FlightDeckQueryPlan::EdgeFind(request),
            })
        }
        SessionCommand::SessionProfileNodeFind {
            model_name, terms, ..
        } => {
            let request = bounded_node_find_request(model_name, terms, default_limit)?;
            reject_edge_return(&request)?;
            Ok(ParsedFlightDeckQuery {
                command: command.into(),
                kind: FlightDeckQueryKind::Profile,
                plan: FlightDeckQueryPlan::NodeFind(request),
            })
        }
        SessionCommand::SessionProfileEdgeFind {
            model_name, terms, ..
        } => {
            let request = bounded_edge_find_request(model_name, terms, default_limit)?;
            Ok(ParsedFlightDeckQuery {
                command: command.into(),
                kind: FlightDeckQueryKind::Profile,
                plan: FlightDeckQueryPlan::EdgeFind(request),
            })
        }
        _ => Err("unsupported read-only command; use node.find, edge.find, session.explain, or session.profile".into()),
    }
}

fn reject_unsafe_command_text(command: &str) -> Result<(), String> {
    let trimmed = command.trim();
    let first = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        first.as_str(),
        "schema.define"
            | "model.define"
            | "link.define"
            | "node.create"
            | "node.update"
            | "node.edit"
            | "node.delete"
            | "edge.create"
            | "edge.update"
            | "edge.edit"
            | "edge.delete"
            | "batch"
            | "session.import"
            | "session.load"
            | "session.save"
    ) {
        return Err(
            "write, durability, and admin commands are not supported in the flight-deck query bar"
                .into(),
        );
    }

    if ["MATCH", "CREATE", "MERGE", "SET", "DELETE"]
        .into_iter()
        .any(|keyword| {
            trimmed
                .to_ascii_uppercase()
                .split_whitespace()
                .any(|token| token == keyword)
        })
    {
        return Err(
            "Cypher-like commands are deferred; use the read-only GRM syntax subset".into(),
        );
    }

    Ok(())
}

fn bounded_node_find_request(
    model_name: String,
    terms: Vec<QueryTerm>,
    default_limit: usize,
) -> Result<NodeFindRequest, String> {
    let mut request = NodeFindRequest::from_adapter_query_terms(model_name, terms)
        .map_err(|error| error.to_string())?;
    request.limit = Some(request.limit.unwrap_or(default_limit).clamp(1, 1_000));
    if let Some(offset) = request.offset {
        request.offset = Some(offset.min(100_000));
    }
    Ok(request)
}

fn bounded_edge_find_request(
    model_name: String,
    terms: Vec<QueryTerm>,
    default_limit: usize,
) -> Result<EdgeFindRequest, String> {
    let filters = terms
        .into_iter()
        .map(|term| (term.key, serde_json::Value::String(term.value)))
        .collect::<BTreeMap<_, _>>();
    let mut request = EdgeFindRequest::from_adapter_filter_values(model_name, filters)
        .map_err(|error| error.to_string())?;
    request.limit = Some(request.limit.unwrap_or(default_limit).clamp(1, 1_000));
    if let Some(offset) = request.offset {
        request.offset = Some(offset.min(100_000));
    }
    Ok(request)
}

fn reject_edge_return(request: &NodeFindRequest) -> Result<(), String> {
    if request.return_mode == Some(grm_rs::TraversalReturn::Edge) {
        return Err("node.find return=edge is intentionally deferred in this gateway slice".into());
    }
    Ok(())
}

async fn collect_snapshot(
    client: &mut GrpcWorkspaceClient,
    workspace: String,
    model_limit: usize,
) -> Result<FlightDeckSnapshot, GrpcWorkspaceClientError> {
    let schema = client.schema_list().await?;
    let node_models = schema
        .node_models
        .iter()
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    let edge_models = schema
        .edge_models
        .iter()
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    let schema_edges = schema
        .edge_models
        .iter()
        .map(|model| FlightDeckSchemaEdge {
            model: model.name.clone(),
            from_model: model.from_model.clone(),
            to_model: model.to_model.clone(),
        })
        .collect::<Vec<_>>();
    let schema_node_models = schema
        .node_models
        .iter()
        .map(|model| FlightDeckSchemaNodeModel {
            name: model.name.clone(),
            id_field: model.id_field_name.clone(),
            fields: schema_fields(&model.fields),
        })
        .collect::<Vec<_>>();
    let schema_edge_models = schema
        .edge_models
        .iter()
        .map(|model| FlightDeckSchemaEdgeModel {
            name: model.name.clone(),
            from_model: model.from_model.clone(),
            to_model: model.to_model.clone(),
            id_field: model.id_field_name.clone(),
            fields: schema_fields(&model.fields),
        })
        .collect::<Vec<_>>();

    let mut nodes = BTreeMap::new();
    for model in &node_models {
        let found = client
            .find_nodes(NodeFindRequest {
                model: model.clone(),
                limit: Some(model_limit),
                ..Default::default()
            })
            .await?;
        for node in found.nodes {
            nodes.insert(node.id, node);
        }
    }

    let mut edges = BTreeMap::new();
    let mut omitted_edges = 0;
    for model in &edge_models {
        let found = client
            .find_edges(EdgeFindRequest {
                model: model.clone(),
                limit: Some(model_limit),
                ..Default::default()
            })
            .await?;
        let (included, omitted) = include_edges_with_known_endpoints(&nodes, found.edges);
        edges.extend(included);
        omitted_edges += omitted;
    }

    Ok(FlightDeckSnapshot {
        workspace,
        node_models,
        edge_models,
        schema_node_models,
        schema_edge_models,
        schema_edges,
        nodes: nodes.into_values().map(flight_deck_node).collect(),
        edges: edges.into_values().map(flight_deck_edge).collect(),
        model_limit,
        omitted_edges,
    })
}

async fn edge_result_with_known_endpoints(
    client: &mut GrpcWorkspaceClient,
    schema_edge_models: &[FlightDeckSchemaEdgeModel],
    request: &EdgeFindRequest,
    edges: Vec<StoredRel>,
) -> Result<(Vec<FlightDeckNode>, Vec<FlightDeckEdge>, usize), GrpcWorkspaceClientError> {
    let Some(schema_edge) = schema_edge_models
        .iter()
        .find(|model| model.name == request.model)
    else {
        return Ok((Vec::new(), Vec::new(), edges.len()));
    };

    let mut nodes = BTreeMap::new();
    let mut omitted_edges = 0;
    for edge in &edges {
        let from = client
            .find_nodes(NodeFindRequest {
                model: schema_edge.from_model.clone(),
                id: Some(edge.from),
                limit: Some(1),
                ..Default::default()
            })
            .await?;
        let to = client
            .find_nodes(NodeFindRequest {
                model: schema_edge.to_model.clone(),
                id: Some(edge.to),
                limit: Some(1),
                ..Default::default()
            })
            .await?;
        if let Some(node) = from.nodes.into_iter().next() {
            nodes.insert(node.id, node);
        }
        if let Some(node) = to.nodes.into_iter().next() {
            nodes.insert(node.id, node);
        }
        if !nodes.contains_key(&edge.from) || !nodes.contains_key(&edge.to) {
            omitted_edges += 1;
        }
    }

    let known_node_ids = nodes.keys().copied().collect::<BTreeSet<_>>();
    let graph_edges = edges
        .into_iter()
        .filter(|edge| known_node_ids.contains(&edge.from) && known_node_ids.contains(&edge.to))
        .map(flight_deck_edge)
        .collect::<Vec<_>>();

    Ok((
        nodes.into_values().map(flight_deck_node).collect(),
        graph_edges,
        omitted_edges,
    ))
}

async fn service_explain_evidence(
    client: &mut GrpcWorkspaceClient,
    plan: &FlightDeckQueryPlan,
) -> Result<FlightDeckQueryEvidence, GrpcWorkspaceClientError> {
    let response = client
        .explain(ExplainRequest {
            query: runtime_query_request(plan),
        })
        .await?;
    Ok(FlightDeckQueryEvidence {
        provenance: "service".into(),
        label: "service/runtime explain evidence".into(),
        plan_kind: Some(response.plan_kind),
        steps: response.steps,
        indexes: response.indexes,
        row_count: None,
        elapsed_micros: None,
        unsupported_reason: None,
    })
}

async fn service_profile_evidence(
    client: &mut GrpcWorkspaceClient,
    plan: &FlightDeckQueryPlan,
) -> Result<FlightDeckQueryEvidence, GrpcWorkspaceClientError> {
    let response = client
        .profile(ProfileRequest {
            query: runtime_query_request(plan),
        })
        .await?;
    let plan = response.plan.unwrap_or_default();
    Ok(FlightDeckQueryEvidence {
        provenance: "service".into(),
        label: "service/runtime profile evidence".into(),
        plan_kind: Some(plan.plan_kind),
        steps: plan.steps,
        indexes: plan.indexes,
        row_count: Some(response.row_count),
        elapsed_micros: Some(response.elapsed_micros),
        unsupported_reason: None,
    })
}

fn runtime_query_request(plan: &FlightDeckQueryPlan) -> QueryRequest {
    match plan {
        FlightDeckQueryPlan::NodeFind(request) => QueryRequest::NodeFind(request.clone()),
        FlightDeckQueryPlan::EdgeFind(request) => QueryRequest::EdgeFind(request.clone()),
    }
}

fn query_shape(plan: &FlightDeckQueryPlan) -> &'static str {
    match plan {
        FlightDeckQueryPlan::NodeFind(_) => "node.find",
        FlightDeckQueryPlan::EdgeFind(_) => "edge.find",
    }
}

fn query_limit(plan: &FlightDeckQueryPlan) -> Option<usize> {
    match plan {
        FlightDeckQueryPlan::NodeFind(request) => request.limit,
        FlightDeckQueryPlan::EdgeFind(request) => request.limit,
    }
}

fn schema_fields(fields: &[RuntimeField]) -> Vec<FlightDeckSchemaField> {
    fields
        .iter()
        .map(|field| FlightDeckSchemaField {
            name: field.name.clone(),
            value_type: value_type_label(&field.value_type).into(),
            required: field.required,
        })
        .collect()
}

fn value_type_label(value_type: &RuntimeValueType) -> &'static str {
    match value_type {
        RuntimeValueType::String => "string",
        RuntimeValueType::Int => "int",
        RuntimeValueType::Float => "float",
        RuntimeValueType::Bool => "bool",
    }
}

async fn close_after_snapshot(
    snapshot_result: Result<FlightDeckSnapshot, GrpcWorkspaceClientError>,
    close: impl Future<Output = Result<(), GrpcWorkspaceClientError>>,
) -> Result<FlightDeckSnapshot, SnapshotLoadError> {
    match snapshot_result {
        Ok(snapshot) => {
            close.await.map_err(SnapshotLoadError::Close)?;
            Ok(snapshot)
        }
        Err(primary) => {
            let close_error = close.await.err().map(|error| error.to_string());
            Err(SnapshotLoadError::Snapshot {
                primary,
                close_error,
            })
        }
    }
}

fn include_edges_with_known_endpoints(
    nodes: &BTreeMap<i64, StoredNode>,
    edges: Vec<StoredRel>,
) -> (BTreeMap<i64, StoredRel>, usize) {
    let included_node_ids = nodes.keys().copied().collect::<BTreeSet<_>>();
    let mut included = BTreeMap::new();
    let mut omitted = 0;
    for edge in edges {
        if included_node_ids.contains(&edge.from) && included_node_ids.contains(&edge.to) {
            included.insert(edge.id, edge);
        } else {
            omitted += 1;
        }
    }
    (included, omitted)
}

fn flight_deck_node(node: StoredNode) -> FlightDeckNode {
    FlightDeckNode {
        id: node.id.to_string(),
        model: node
            .labels
            .first()
            .cloned()
            .unwrap_or_else(|| "Node".into()),
        label: node_label(&node),
        props: node.props,
    }
}

fn flight_deck_edge(edge: StoredRel) -> FlightDeckEdge {
    FlightDeckEdge {
        id: edge.id.to_string(),
        model: edge.rel_type,
        from: edge.from.to_string(),
        to: edge.to.to_string(),
        props: edge.props,
    }
}

#[derive(Debug)]
enum SnapshotLoadError {
    Snapshot {
        primary: GrpcWorkspaceClientError,
        close_error: Option<String>,
    },
    Close(GrpcWorkspaceClientError),
}

#[derive(Debug)]
enum QueryLoadError {
    Query {
        primary: GrpcWorkspaceClientError,
        close_error: Option<String>,
    },
    Close(GrpcWorkspaceClientError),
}

impl fmt::Display for QueryLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query {
                primary,
                close_error,
            } => {
                write!(f, "{primary}")?;
                if let Some(close_error) = close_error {
                    write!(
                        f,
                        "; additionally failed to close workspace handle after query error: {close_error}"
                    )?;
                }
                Ok(())
            }
            Self::Close(error) => write!(f, "failed to close workspace handle: {error}"),
        }
    }
}

impl std::error::Error for QueryLoadError {}

impl From<GrpcWorkspaceClientError> for QueryLoadError {
    fn from(error: GrpcWorkspaceClientError) -> Self {
        Self::Query {
            primary: error,
            close_error: None,
        }
    }
}

impl fmt::Display for SnapshotLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot {
                primary,
                close_error,
            } => {
                write!(f, "{primary}")?;
                if let Some(close_error) = close_error {
                    write!(
                        f,
                        "; additionally failed to close workspace handle after snapshot error: {close_error}"
                    )?;
                }
                Ok(())
            }
            Self::Close(error) => write!(f, "failed to close workspace handle: {error}"),
        }
    }
}

impl std::error::Error for SnapshotLoadError {}

impl From<GrpcWorkspaceClientError> for SnapshotLoadError {
    fn from(error: GrpcWorkspaceClientError) -> Self {
        Self::Snapshot {
            primary: error,
            close_error: None,
        }
    }
}

fn node_label(node: &StoredNode) -> String {
    let model = node.labels.first().map(String::as_str).unwrap_or("Node");
    let identifying_value = ["name", "title", "path", "question", "text"]
        .into_iter()
        .find_map(|field| node.props.get(field))
        .and_then(|value| value.as_str());

    match identifying_value {
        Some(value) => format!("{model}: {value}"),
        None => format!("{model} #{}", node.id),
    }
}

fn configured_flight_deck_allowed_origins() -> Result<Vec<HeaderValue>, String> {
    let raw = std::env::var("GRM_FLIGHT_DECK_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| DEFAULT_FLIGHT_DECK_ALLOWED_ORIGINS.into());
    parse_flight_deck_allowed_origins(&raw)
}

fn parse_flight_deck_allowed_origins(raw: &str) -> Result<Vec<HeaderValue>, String> {
    let origins = raw
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            HeaderValue::from_str(origin)
                .map_err(|_| format!("invalid allowed flight-deck origin '{origin}'"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if origins.is_empty() {
        return Err("at least one flight-deck UI origin must be allowed".into());
    }

    Ok(origins)
}

fn flight_deck_cors_layer(allowed_origins: Vec<HeaderValue>) -> CorsLayer {
    let allowed_origins = Arc::new(allowed_origins);
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _request_parts| {
            origin_is_allowed(origin, allowed_origins.as_ref())
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE])
}

fn origin_is_allowed(origin: &HeaderValue, allowed_origins: &[HeaderValue]) -> bool {
    allowed_origins.iter().any(|allowed| allowed == origin)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let grpc_endpoint =
        std::env::var("GRM_SERVICE_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let bind = std::env::var("GRM_FLIGHT_DECK_HTTP_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3001".into())
        .parse::<SocketAddr>()?;
    let cors = flight_deck_cors_layer(
        configured_flight_deck_allowed_origins()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?,
    );
    let app = Router::new()
        .route("/api/security/status", get(security_status))
        .route("/api/workspaces/:workspace/snapshot", get(snapshot))
        .route("/api/workspaces/:workspace/query", post(query_command))
        .with_state(AppState {
            grpc_endpoint: grpc_endpoint.into(),
        })
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    println!("GRM flight-deck HTTP adapter listening on http://{bind}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use grm_rs::{StoredNode, StoredRel};
    use grm_service_api::{GrpcTlsConfigError, GrpcWorkspaceClientError};
    use serde_json::json;

    use super::{
        close_after_snapshot, flight_deck_edge, flight_deck_node, flight_deck_security_status,
        include_edges_with_known_endpoints, node_label, origin_is_allowed,
        parse_flight_deck_allowed_origins, redacted_gateway_error, redacted_gateway_log_message,
    };

    #[test]
    fn node_label_prefers_a_semantic_lookup_field() {
        let node = StoredNode {
            id: 7,
            labels: vec!["Decision".into()],
            props: BTreeMap::from([("title".into(), json!("Use typed operations"))]),
        };

        assert_eq!(node_label(&node), "Decision: Use typed operations");
    }

    #[test]
    fn node_label_falls_back_to_model_and_id() {
        let node = StoredNode {
            id: 9,
            labels: vec!["Decision".into()],
            props: BTreeMap::new(),
        };

        assert_eq!(node_label(&node), "Decision #9");
    }

    #[test]
    fn flight_deck_ids_are_serialized_as_strings() {
        let node = flight_deck_node(StoredNode {
            id: i64::MAX,
            labels: vec!["LargeId".into()],
            props: BTreeMap::new(),
        });
        let edge = flight_deck_edge(StoredRel {
            id: i64::MAX - 1,
            rel_type: "LINKS".into(),
            from: i64::MAX - 2,
            to: i64::MAX - 3,
            props: BTreeMap::new(),
        });

        assert_eq!(node.id, i64::MAX.to_string());
        assert_eq!(edge.id, (i64::MAX - 1).to_string());
        assert_eq!(edge.from, (i64::MAX - 2).to_string());
        assert_eq!(edge.to, (i64::MAX - 3).to_string());
    }

    #[test]
    fn omitted_edges_count_edges_outside_the_bounded_node_set() {
        let nodes = BTreeMap::from([
            (
                1,
                StoredNode {
                    id: 1,
                    labels: vec!["User".into()],
                    props: BTreeMap::new(),
                },
            ),
            (
                2,
                StoredNode {
                    id: 2,
                    labels: vec!["Post".into()],
                    props: BTreeMap::new(),
                },
            ),
        ]);
        let edges = vec![
            StoredRel {
                id: 10,
                rel_type: "Authored".into(),
                from: 1,
                to: 2,
                props: BTreeMap::new(),
            },
            StoredRel {
                id: 11,
                rel_type: "Authored".into(),
                from: 1,
                to: 99,
                props: BTreeMap::new(),
            },
        ];

        let (included, omitted) = include_edges_with_known_endpoints(&nodes, edges);

        assert_eq!(included.keys().copied().collect::<Vec<_>>(), vec![10]);
        assert_eq!(omitted, 1);
    }

    #[tokio::test]
    async fn close_is_attempted_after_snapshot_error_without_hiding_primary_error() {
        let primary = GrpcWorkspaceClientError::MissingField("schema");
        let close = async { Err(GrpcWorkspaceClientError::UnexpectedResponse("close")) };

        let error = close_after_snapshot(Err(primary), close)
            .await
            .expect_err("snapshot error should be returned");
        let message = error.to_string();

        assert!(message.contains("schema"));
        assert!(message.contains("additionally failed to close workspace handle"));
        assert!(message.contains("close"));
    }

    #[tokio::test]
    async fn close_error_after_success_is_reported() {
        let snapshot = super::FlightDeckSnapshot {
            workspace: "demo".into(),
            node_models: Vec::new(),
            edge_models: Vec::new(),
            schema_node_models: Vec::new(),
            schema_edge_models: Vec::new(),
            schema_edges: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            model_limit: 1,
            omitted_edges: 0,
        };
        let close = async { Err(GrpcWorkspaceClientError::UnexpectedResponse("close")) };

        let error = close_after_snapshot(Ok(snapshot), close)
            .await
            .expect_err("close error should be returned");

        assert!(
            error
                .to_string()
                .contains("failed to close workspace handle")
        );
    }

    #[test]
    fn security_status_serializes_safe_profile_metadata() {
        let status = flight_deck_security_status(super::proto::SecurityStatusResponse {
            security_profile: super::proto::SecurityProfile::Secured as i32,
            identity_status: super::proto::SecurityIdentityStatus::AuthenticatedPrincipal as i32,
            principal: Some(super::proto::SecurityPrincipal {
                issuer: "local-admin".into(),
                subject: "admin-1".into(),
            }),
            authentication_method: "mtls-certificate".into(),
            policy_version: "secured-local-policy-v1".into(),
        });

        assert_eq!(status.security_profile, "secured");
        assert_eq!(status.identity_status, "authenticated_principal");
        assert_eq!(
            status.principal,
            Some(super::FlightDeckPrincipal {
                issuer: "local-admin".into(),
                subject: "admin-1".into(),
            })
        );
        assert_eq!(
            status.authentication_method.as_deref(),
            Some("mtls-certificate")
        );
        assert_eq!(
            status.policy_version.as_deref(),
            Some("secured-local-policy-v1")
        );
    }

    #[test]
    fn public_gateway_errors_do_not_include_local_tls_paths() {
        let path = PathBuf::from("/home/laurie/.grm/secured-local/admin-1.key");
        let error = GrpcWorkspaceClientError::TlsConfig(GrpcTlsConfigError::ReadFile {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing test file"),
        });

        let (status, body) = redacted_gateway_error("security status", &error);
        let log = redacted_gateway_log_message("security status");

        assert_eq!(status, super::StatusCode::BAD_GATEWAY);
        assert_eq!(body, "failed to load security status from GRM service");
        assert!(!body.contains(path.to_string_lossy().as_ref()));
        assert!(!body.contains("admin-1.key"));
        assert!(!body.contains("GRM_SERVICE_TLS_CLIENT_KEY"));
        assert!(!log.contains(path.to_string_lossy().as_ref()));
        assert!(!log.contains("admin-1.key"));
        assert!(!log.contains("GRM_SERVICE_TLS_CLIENT_KEY"));
    }

    #[test]
    fn default_cors_origins_allow_local_flight_deck_ui_only() {
        let origins = parse_flight_deck_allowed_origins(super::DEFAULT_FLIGHT_DECK_ALLOWED_ORIGINS)
            .expect("default origins should parse");
        let vite_origin = super::HeaderValue::from_static("http://127.0.0.1:8081");
        let gateway_origin = super::HeaderValue::from_static("http://127.0.0.1:3001");
        let hostile_origin = super::HeaderValue::from_static("https://attacker.example");

        assert!(origin_is_allowed(&vite_origin, &origins));
        assert!(origin_is_allowed(&gateway_origin, &origins));
        assert!(!origin_is_allowed(&hostile_origin, &origins));
    }

    #[test]
    fn configured_cors_origins_do_not_grant_unlisted_origins() {
        let origins =
            parse_flight_deck_allowed_origins("http://127.0.0.1:8081,http://localhost:8081")
                .expect("configured origins should parse");
        let allowed_origin = super::HeaderValue::from_static("http://localhost:8081");
        let disallowed_origin = super::HeaderValue::from_static("http://evil.localhost:8081");

        assert!(origin_is_allowed(&allowed_origin, &origins));
        assert!(!origin_is_allowed(&disallowed_origin, &origins));
    }

    #[test]
    fn cors_origin_configuration_rejects_empty_or_invalid_values() {
        let empty_error = parse_flight_deck_allowed_origins(" , ")
            .expect_err("empty origin configuration should be rejected");
        let invalid_error = parse_flight_deck_allowed_origins("http://127.0.0.1:8081\nbad")
            .expect_err("invalid origin should be rejected");

        assert!(empty_error.contains("at least one"));
        assert!(invalid_error.contains("invalid allowed flight-deck origin"));
    }

    #[test]
    fn parses_node_find_command_into_a_bounded_typed_request() {
        let parsed =
            super::parse_read_only_query_command("node.find WorkSlice status=active limit=25", 50)
                .expect("node.find should parse");

        assert_eq!(parsed.kind, super::FlightDeckQueryKind::Query);
        match parsed.plan {
            super::FlightDeckQueryPlan::NodeFind(request) => {
                assert_eq!(request.model, "WorkSlice");
                assert_eq!(request.limit, Some(25));
                assert_eq!(request.predicates.len(), 1);
            }
            super::FlightDeckQueryPlan::EdgeFind(_) => panic!("expected node.find"),
        }
    }

    #[test]
    fn parses_session_profile_edge_find_into_a_typed_request() {
        let parsed = super::parse_read_only_query_command(
            "session.profile edge.find HAS_WORK_SLICE from=13 limit=5",
            50,
        )
        .expect("profile edge.find should parse");

        assert_eq!(parsed.kind, super::FlightDeckQueryKind::Profile);
        match parsed.plan {
            super::FlightDeckQueryPlan::EdgeFind(request) => {
                assert_eq!(request.model, "HAS_WORK_SLICE");
                assert_eq!(request.from, Some(13));
                assert_eq!(request.limit, Some(5));
            }
            super::FlightDeckQueryPlan::NodeFind(_) => panic!("expected edge.find"),
        }
    }

    #[test]
    fn rejects_write_and_cypher_like_commands_before_service_execution() {
        let write_error = super::parse_read_only_query_command("node.delete WorkSlice 573", 50)
            .expect_err("write command should be rejected");
        let cypher_error = super::parse_read_only_query_command("MATCH (n) RETURN n LIMIT 5", 50)
            .expect_err("Cypher-like command should be rejected");

        assert!(write_error.contains("write"));
        assert!(cypher_error.contains("Cypher"));
    }

    #[test]
    fn rejects_deferred_node_find_edge_return() {
        let error = super::parse_read_only_query_command(
            "node.find WorkSlice via=out:HAS_WORK_SLICE:AcceptanceCriteria return=edge",
            50,
        )
        .expect_err("return=edge should be deferred");

        assert!(error.contains("return=edge"));
    }
}
