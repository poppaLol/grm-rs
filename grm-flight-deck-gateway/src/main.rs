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
    http::{Method, StatusCode},
    routing::get,
};
use grm_rs::{EdgeFindRequest, NodeFindRequest, StoredNode, StoredRel};
use grm_service_api::{
    DurabilityFormat, GrpcClientTlsOptions, GrpcWorkspaceClient, GrpcWorkspaceClientError,
    GrpcWorkspaceMode, grpc_security_status, proto,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::cors::{Any, CorsLayer};

const DEFAULT_MODEL_LIMIT: usize = 200;

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
        schema_edges,
        nodes: nodes.into_values().map(flight_deck_node).collect(),
        edges: edges.into_values().map(flight_deck_edge).collect(),
        model_limit,
        omitted_edges,
    })
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let grpc_endpoint =
        std::env::var("GRM_SERVICE_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let bind = std::env::var("GRM_FLIGHT_DECK_HTTP_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3001".into())
        .parse::<SocketAddr>()?;
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET]);
    let app = Router::new()
        .route("/api/security/status", get(security_status))
        .route("/api/workspaces/:workspace/snapshot", get(snapshot))
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
        include_edges_with_known_endpoints, node_label, redacted_gateway_error,
        redacted_gateway_log_message,
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
}
