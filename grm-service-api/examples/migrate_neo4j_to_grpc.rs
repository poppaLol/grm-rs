use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::path::PathBuf;

use grm_rs::{
    GraphBackend, KernelValue, Neo4jBackend, Neo4jConfig, RuntimeField, RuntimeNodeModel,
    RuntimeRelModel, RuntimeValueType, SessionState, StoredNode, StoredRel,
};
use grm_service_api::proto;
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse()?;
    let mut state = SessionState::new();
    state.load_from_json(&options.schema)?;

    let node_models = state
        .catalog()
        .list_node_models()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let rel_models = state
        .catalog()
        .list_rel_models()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();

    if node_models.is_empty() && rel_models.is_empty() {
        return Err(format!(
            "schema file '{}' did not contain runtime schema models",
            options.schema.display()
        )
        .into());
    }

    validate_service_schema_shape(&node_models, &rel_models)?;

    let neo4j = Neo4jBackend::connect(Neo4jConfig {
        uri: required_env("NEO4J_URI")?,
        user: required_env("NEO4J_USER")?,
        password: required_env("NEO4J_PASSWORD")?,
    })
    .await?;

    let mut service =
        proto::grm_service_client::GrmServiceClient::connect(options.endpoint.clone()).await?;
    let workspace = proto::WorkspaceRef {
        id: options.workspace.clone(),
    };
    let handle = match options.mode {
        WorkspaceMode::Create => service
            .create_workspace(proto::WorkspaceCreateRequest {
                mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
                workspace: Some(workspace),
                format: options.format as i32,
            })
            .await?
            .into_inner()
            .handle
            .ok_or("WorkspaceCreateResponse.handle missing")?,
        WorkspaceMode::Open => service
            .open_workspace(proto::WorkspaceOpenRequest {
                snapshot: None,
                workspace: Some(workspace),
                format: options.format as i32,
            })
            .await?
            .into_inner()
            .handle
            .ok_or("WorkspaceOpenResponse.handle missing")?,
    };

    println!(
        "Migrating Neo4j graph into gRPC workspace '{}' at {}",
        options.workspace, options.endpoint
    );
    println!(
        "Schema: {} node models, {} edge models",
        node_models.len(),
        rel_models.len()
    );

    for model in &node_models {
        execute_workspace(
            &mut service,
            &handle,
            proto::runtime_request::Request::DefineNode(proto::DefineNodeRequest {
                name: model.name.clone(),
                id_field: model.id_field_name.clone(),
                fields: proto_fields(&model.fields),
            }),
        )
        .await?;
    }

    for model in &rel_models {
        execute_workspace(
            &mut service,
            &handle,
            proto::runtime_request::Request::DefineEdge(proto::DefineEdgeRequest {
                name: model.name.clone(),
                from_model: model.from_model.clone(),
                to_model: model.to_model.clone(),
                id_field: model.id_field_name.clone(),
                fields: proto_fields(&model.fields),
            }),
        )
        .await?;
    }

    let mut id_map = BTreeMap::<i64, i64>::new();
    let mut migrated_nodes = 0usize;
    for model in &node_models {
        let nodes = read_neo4j_nodes(&neo4j, model).await?;
        println!("{}: {} nodes", model.name, nodes.len());
        for node in nodes {
            let created = execute_workspace(
                &mut service,
                &handle,
                proto::runtime_request::Request::CreateNode(proto::NodeCreateRequest {
                    model: model.name.clone(),
                    props: Some(proto_property_map(node.props)?),
                }),
            )
            .await?;
            let new_id = created_node_id(created)?;
            id_map.insert(node.id, new_id);
            migrated_nodes += 1;
        }
    }

    let mut migrated_edges = 0usize;
    for model in &rel_models {
        let edges = read_neo4j_edges(&neo4j, model).await?;
        println!("{}: {} edges", model.name, edges.len());
        for edge in edges {
            let Some(from) = id_map.get(&edge.from).copied() else {
                return Err(format!(
                    "edge {}:{} references unmigrated from node {}",
                    model.name, edge.id, edge.from
                )
                .into());
            };
            let Some(to) = id_map.get(&edge.to).copied() else {
                return Err(format!(
                    "edge {}:{} references unmigrated to node {}",
                    model.name, edge.id, edge.to
                )
                .into());
            };
            execute_workspace(
                &mut service,
                &handle,
                proto::runtime_request::Request::CreateEdge(proto::EdgeCreateRequest {
                    model: model.name.clone(),
                    from,
                    to,
                    props: Some(proto_property_map(edge.props)?),
                }),
            )
            .await?;
            migrated_edges += 1;
        }
    }

    service
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await?;

    println!(
        "Migrated {} nodes and {} edges into workspace '{}'.",
        migrated_nodes, migrated_edges, options.workspace
    );
    println!(
        "Restart MCP with GRM_BACKEND=grpc GRM_SERVICE_ENDPOINT={} GRM_WORKSPACE_REF={} GRM_SERVICE_WORKSPACE_MODE=open",
        options.endpoint, options.workspace
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct Options {
    schema: PathBuf,
    endpoint: String,
    workspace: String,
    mode: WorkspaceMode,
    format: proto::DurabilityFormat,
}

#[derive(Debug, Clone, Copy)]
enum WorkspaceMode {
    Create,
    Open,
}

impl Options {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut schema = env::var_os("GRM_SCHEMA_TEMPLATE").map(PathBuf::from);
        let mut endpoint = env::var("GRM_SERVICE_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
        let mut workspace =
            env::var("GRM_WORKSPACE_REF").unwrap_or_else(|_| "neo4j-migration".to_string());
        let mut mode = WorkspaceMode::Create;
        let mut format = proto::DurabilityFormat::Binary;

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--schema" => schema = Some(PathBuf::from(next_arg(&mut args, "--schema")?)),
                "--endpoint" => endpoint = next_arg(&mut args, "--endpoint")?,
                "--workspace" => workspace = next_arg(&mut args, "--workspace")?,
                "--mode" => {
                    mode = match next_arg(&mut args, "--mode")?.as_str() {
                        "create" => WorkspaceMode::Create,
                        "open" => WorkspaceMode::Open,
                        other => return Err(format!("unknown --mode '{other}'").into()),
                    };
                }
                "--format" => {
                    format = match next_arg(&mut args, "--format")?.as_str() {
                        "json" => proto::DurabilityFormat::Json,
                        "bin" | "binary" => proto::DurabilityFormat::Binary,
                        other => return Err(format!("unknown --format '{other}'").into()),
                    };
                }
                "--help" | "-h" => return Err(usage().into()),
                other => return Err(format!("unknown argument '{other}'\n{}", usage()).into()),
            }
        }

        Ok(Self {
            schema: schema.ok_or("set --schema or GRM_SCHEMA_TEMPLATE")?,
            endpoint,
            workspace,
            mode,
            format,
        })
    }
}

fn usage() -> &'static str {
    "usage: cargo run -p grm-service-api --features neo4j --example migrate_neo4j_to_grpc -- \
     --schema <schema-session.json> --endpoint http://127.0.0.1:50051 \
     --workspace <workspace-ref> [--mode create|open] [--format json|bin; default bin]"
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("{name} must be set").into())
}

fn validate_service_schema_shape(
    node_models: &[RuntimeNodeModel],
    rel_models: &[RuntimeRelModel],
) -> Result<(), Box<dyn Error>> {
    for model in node_models {
        if model.label != model.name {
            return Err(format!(
                "node model '{}' uses label '{}'; DefineNode cannot preserve custom labels yet",
                model.name, model.label
            )
            .into());
        }
    }
    for model in rel_models {
        if model.rel_type != model.name {
            return Err(format!(
                "edge model '{}' uses rel_type '{}'; DefineEdge cannot preserve custom rel types yet",
                model.name, model.rel_type
            )
            .into());
        }
    }
    Ok(())
}

async fn read_neo4j_nodes(
    neo4j: &Neo4jBackend,
    model: &RuntimeNodeModel,
) -> Result<Vec<StoredNode>, Box<dyn Error>> {
    let query = format!(
        "MATCH (n:{}) RETURN n ORDER BY id(n)",
        cypher_identifier(&model.label)?
    );
    let result = neo4j
        .execute_query(&query, Value::Object(Default::default()))
        .await?;
    result
        .rows
        .into_iter()
        .map(|row| match row.values.into_values().next() {
            Some(KernelValue::Node(node)) => Ok(StoredNode {
                id: node.id,
                labels: node.labels,
                props: node.props,
            }),
            other => Err(format!("Neo4j query for '{}' returned {other:?}", model.name).into()),
        })
        .collect()
}

async fn read_neo4j_edges(
    neo4j: &Neo4jBackend,
    model: &RuntimeRelModel,
) -> Result<Vec<StoredRel>, Box<dyn Error>> {
    let query = format!(
        "MATCH ()-[r:{}]->() RETURN r ORDER BY id(r)",
        cypher_identifier(&model.rel_type)?
    );
    let result = neo4j
        .execute_query(&query, Value::Object(Default::default()))
        .await?;
    result
        .rows
        .into_iter()
        .map(|row| match row.values.into_values().next() {
            Some(KernelValue::Rel(rel)) => Ok(StoredRel {
                id: rel.id,
                rel_type: rel.ty,
                from: rel.from,
                to: rel.to,
                props: rel.props,
            }),
            other => Err(format!("Neo4j query for '{}' returned {other:?}", model.name).into()),
        })
        .collect()
}

async fn execute_workspace(
    client: &mut proto::grm_service_client::GrmServiceClient<tonic::transport::Channel>,
    handle: &proto::WorkspaceHandle,
    request: proto::runtime_request::Request,
) -> Result<proto::WorkspaceRuntimeResponse, Box<dyn Error>> {
    let response = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(request),
            }),
        })
        .await?
        .into_inner();
    Ok(response)
}

fn created_node_id(response: proto::WorkspaceRuntimeResponse) -> Result<i64, Box<dyn Error>> {
    match response.response.and_then(|response| response.response) {
        Some(proto::runtime_response::Response::CreateNode(response)) => {
            Ok(response.node.ok_or("NodeCreateResponse.node missing")?.id)
        }
        other => Err(format!("expected create node response, got {other:?}").into()),
    }
}

fn proto_fields(fields: &[RuntimeField]) -> Vec<proto::FieldSpec> {
    fields
        .iter()
        .map(|field| proto::FieldSpec {
            name: field.name.clone(),
            value_type: proto_value_type(&field.value_type) as i32,
            required: field.required,
        })
        .collect()
}

fn proto_value_type(value_type: &RuntimeValueType) -> proto::FieldValueType {
    match value_type {
        RuntimeValueType::String => proto::FieldValueType::String,
        RuntimeValueType::Int => proto::FieldValueType::Int,
        RuntimeValueType::Float => proto::FieldValueType::Float,
        RuntimeValueType::Bool => proto::FieldValueType::Bool,
    }
}

fn proto_property_map(
    props: BTreeMap<String, Value>,
) -> Result<proto::PropertyMap, Box<dyn Error>> {
    let properties = props
        .into_iter()
        .map(|(name, value)| {
            Ok(proto::Property {
                name,
                value: Some(proto::PropertyValue {
                    kind: Some(proto_property_value(value)?),
                }),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    Ok(proto::PropertyMap { properties })
}

fn proto_property_value(value: Value) -> Result<proto::property_value::Kind, Box<dyn Error>> {
    match value {
        Value::String(value) => Ok(proto::property_value::Kind::StringValue(value)),
        Value::Bool(value) => Ok(proto::property_value::Kind::BoolValue(value)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(proto::property_value::Kind::IntValue(value))
            } else if let Some(value) = value.as_f64() {
                Ok(proto::property_value::Kind::FloatValue(value))
            } else {
                Err("unsupported unsigned integer property value".into())
            }
        }
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err("graph property values must be strings, numbers, or bools".into())
        }
    }
}

fn cypher_identifier(value: &str) -> Result<&str, Box<dyn Error>> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err("empty Cypher identifier".into());
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!("unsupported Cypher identifier '{value}'").into());
    }
    if chars.any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_')) {
        return Err(format!("unsupported Cypher identifier '{value}'").into());
    }
    Ok(value)
}
