use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{Method, StatusCode},
    routing::get,
};
use grm_rs::{EdgeFindRequest, NodeFindRequest, StoredNode};
use grm_service_api::{GrpcWorkspaceClient, GrpcWorkspaceClientError, GrpcWorkspaceMode};
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
    id: i64,
    model: String,
    label: String,
    props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct FlightDeckEdge {
    id: i64,
    model: String,
    from: i64,
    to: i64,
    props: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightDeckSchemaEdge {
    model: String,
    from_model: String,
    to_model: String,
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
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))
}

async fn load_snapshot(
    endpoint: impl Into<String>,
    workspace: impl Into<String>,
    model_limit: usize,
) -> Result<FlightDeckSnapshot, GrpcWorkspaceClientError> {
    let endpoint = endpoint.into();
    let workspace = workspace.into();
    let mut client =
        GrpcWorkspaceClient::connect(endpoint, workspace.clone(), GrpcWorkspaceMode::Open).await?;

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

    let included_node_ids = nodes.keys().copied().collect::<BTreeSet<_>>();
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
        for edge in found.edges {
            if included_node_ids.contains(&edge.from) && included_node_ids.contains(&edge.to) {
                edges.insert(edge.id, edge);
            } else {
                omitted_edges += 1;
            }
        }
    }

    client.close().await?;

    Ok(FlightDeckSnapshot {
        workspace,
        node_models,
        edge_models,
        schema_edges,
        nodes: nodes
            .into_values()
            .map(|node| FlightDeckNode {
                id: node.id,
                model: node
                    .labels
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Node".into()),
                label: node_label(&node),
                props: node.props,
            })
            .collect(),
        edges: edges
            .into_values()
            .map(|edge| FlightDeckEdge {
                id: edge.id,
                model: edge.rel_type,
                from: edge.from,
                to: edge.to,
                props: edge.props,
            })
            .collect(),
        model_limit,
        omitted_edges,
    })
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

    use grm_rs::StoredNode;
    use serde_json::json;

    use super::node_label;

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
}
