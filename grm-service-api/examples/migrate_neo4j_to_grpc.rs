use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use base64::Engine;
use grm_rs::{
    GraphBackend, KernelValue, Neo4jBackend, Neo4jConfig, RuntimeField, RuntimeNodeModel,
    RuntimeRelModel, RuntimeValueType, SessionState, StoredNode, StoredRel,
};
use grm_service_api::{GrpcClientTlsOptions, grpc_channel, proto};
use serde::{Deserialize, Serialize};
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

    if options.execution == ExecutionMode::Execute
        && matches!(options.mode, WorkspaceMode::Open)
        && !options.allow_open_target
    {
        return Err(
            "--mode open is not safe for migration execution; use a fresh workspace or pass \
             --allow-open-target after operator approval"
                .into(),
        );
    }

    let neo4j = Neo4jBackend::connect(Neo4jConfig {
        uri: required_env("NEO4J_URI")?,
        user: required_env("NEO4J_USER")?,
        password: required_env("NEO4J_PASSWORD")?,
    })
    .await?;

    println!(
        "{} Neo4j graph into gRPC workspace '{}' at {}",
        match options.execution {
            ExecutionMode::DryRun => "Dry-running migration of",
            ExecutionMode::Execute => "Migrating",
        },
        options.workspace,
        options.endpoint
    );
    println!(
        "Schema: {} node models, {} edge models",
        node_models.len(),
        rel_models.len()
    );

    let mut source_nodes = BTreeMap::<String, Vec<StoredNode>>::new();
    let mut source_edges = BTreeMap::<String, Vec<StoredRel>>::new();

    for model in &node_models {
        let nodes = read_neo4j_nodes(&neo4j, model).await?;
        println!("{}: {} nodes", model.name, nodes.len());
        source_nodes.insert(model.name.clone(), nodes);
    }

    for model in &rel_models {
        let edges = read_neo4j_edges(&neo4j, model).await?;
        println!("{}: {} edges", model.name, edges.len());
        source_edges.insert(model.name.clone(), edges);
    }

    let mut report = MigrationReport::new(&options, &node_models, &rel_models);
    if let Some(path) = &options.selection {
        let selection: MigrationSelection = serde_json::from_slice(&fs::read(path)?)?;
        select_source(&selection, &mut source_nodes, &mut source_edges)?;
    }
    preflight_source(
        &node_models,
        &rel_models,
        &source_nodes,
        &source_edges,
        &mut report,
    );
    write_report_if_requested(&options, &report, None)?;

    if report.has_errors() {
        return Err(format!(
            "migration preflight found {} blocking issue(s); see the report before retrying",
            report.errors.len()
        )
        .into());
    }

    if options.execution == ExecutionMode::DryRun {
        println!(
            "Dry run complete: {} source nodes and {} source edges are mappable.",
            report.total_source_nodes, report.total_source_edges
        );
        println!("Pass --execute to create a fresh gRPC workspace after reviewing the report.");
        return Ok(());
    }

    let tls = GrpcClientTlsOptions::from_env()?;
    let channel = grpc_channel(options.endpoint.clone(), tls.as_ref()).await?;
    let mut service = proto::grm_service_client::GrmServiceClient::new(channel);
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
    let mut persisted_map = MigrationIdMap::new(&options);
    let mut migrated_nodes = 0usize;
    for model in &node_models {
        for node in source_nodes.remove(&model.name).unwrap_or_default() {
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
            persisted_map.nodes.push(NodeIdMapping {
                model: model.name.clone(),
                source_id: node.id,
                target_id: new_id,
            });
            migrated_nodes += 1;
        }
    }

    let mut migrated_edges = 0usize;
    for model in &rel_models {
        for edge in source_edges.remove(&model.name).unwrap_or_default() {
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
            persisted_map.edges.push(EdgeIdMapping {
                model: model.name.clone(),
                source_id: edge.id,
                target_from: from,
                target_to: to,
            });
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
    report.total_target_nodes = Some(migrated_nodes);
    report.total_target_edges = Some(migrated_edges);
    write_id_map_if_requested(&options, &persisted_map)?;
    write_report_if_requested(&options, &report, Some(&persisted_map))?;
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
    execution: ExecutionMode,
    allow_open_target: bool,
    report: Option<PathBuf>,
    id_map: Option<PathBuf>,
    selection: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceMode {
    Create,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionMode {
    DryRun,
    Execute,
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
        let mut execution = ExecutionMode::DryRun;
        let mut allow_open_target = false;
        let mut report = None;
        let mut id_map = None;
        let mut selection = None;

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
                "--execute" => execution = ExecutionMode::Execute,
                "--dry-run" => execution = ExecutionMode::DryRun,
                "--allow-open-target" => allow_open_target = true,
                "--report" => report = Some(PathBuf::from(next_arg(&mut args, "--report")?)),
                "--id-map" => id_map = Some(PathBuf::from(next_arg(&mut args, "--id-map")?)),
                "--selection" => {
                    selection = Some(PathBuf::from(next_arg(&mut args, "--selection")?))
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
            execution,
            allow_open_target,
            report,
            id_map,
            selection,
        })
    }
}

fn usage() -> &'static str {
    "usage: cargo run -p grm-service-api --features neo4j --example migrate_neo4j_to_grpc -- \
     --schema <schema-session.json> --endpoint http://127.0.0.1:50051 \
     --workspace <workspace-ref> [--dry-run|--execute] [--mode create|open] \
     [--format json|bin; default bin] [--report report.json] [--id-map map.json] \
     [--selection selection.json]"
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationSelection {
    node_ids: Vec<i64>,
    edge_ids: Vec<i64>,
}

fn select_source(
    selection: &MigrationSelection,
    nodes: &mut BTreeMap<String, Vec<StoredNode>>,
    edges: &mut BTreeMap<String, Vec<StoredRel>>,
) -> Result<(), Box<dyn Error>> {
    let node_ids: BTreeSet<_> = selection.node_ids.iter().copied().collect();
    let edge_ids: BTreeSet<_> = selection.edge_ids.iter().copied().collect();
    if node_ids.is_empty()
        || node_ids.len() != selection.node_ids.len()
        || edge_ids.len() != selection.edge_ids.len()
    {
        return Err(
            "selection must contain nodes and must not contain duplicate node or edge IDs".into(),
        );
    }
    let known_nodes: BTreeSet<_> = nodes.values().flatten().map(|node| node.id).collect();
    let known_edges: BTreeSet<_> = edges.values().flatten().map(|edge| edge.id).collect();
    if !node_ids.is_subset(&known_nodes) || !edge_ids.is_subset(&known_edges) {
        return Err("selection references IDs absent from the declared source models".into());
    }
    for edge in edges
        .values()
        .flatten()
        .filter(|edge| edge_ids.contains(&edge.id))
    {
        if !node_ids.contains(&edge.from) || !node_ids.contains(&edge.to) {
            return Err(format!("selected edge {} has an unselected endpoint", edge.id).into());
        }
    }
    for records in nodes.values_mut() {
        records.retain(|node| node_ids.contains(&node.id));
    }
    for records in edges.values_mut() {
        records.retain(|edge| edge_ids.contains(&edge.id));
    }
    Ok(())
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

fn preflight_source(
    node_models: &[RuntimeNodeModel],
    rel_models: &[RuntimeRelModel],
    source_nodes: &BTreeMap<String, Vec<StoredNode>>,
    source_edges: &BTreeMap<String, Vec<StoredRel>>,
    report: &mut MigrationReport,
) {
    let node_by_model = node_models
        .iter()
        .map(|model| (model.name.as_str(), model))
        .collect::<BTreeMap<_, _>>();
    let rel_by_model = rel_models
        .iter()
        .map(|model| (model.name.as_str(), model))
        .collect::<BTreeMap<_, _>>();
    let mut node_seen = BTreeMap::<i64, Vec<String>>::new();
    let mut mappable_node_ids = BTreeSet::<i64>::new();
    let mut ids_by_model = BTreeMap::<&str, BTreeSet<i64>>::new();

    for (model_name, nodes) in source_nodes {
        let Some(model) = node_by_model.get(model_name.as_str()) else {
            report
                .errors
                .push(format!("source nodes for undeclared model '{model_name}'"));
            continue;
        };
        report.node_counts.insert(model_name.clone(), nodes.len());
        for node in nodes {
            node_seen
                .entry(node.id)
                .or_default()
                .push(model_name.clone());
            mappable_node_ids.insert(node.id);
            ids_by_model
                .entry(model_name.as_str())
                .or_default()
                .insert(node.id);
            if !node.labels.iter().any(|label| label == &model.label) {
                report.errors.push(format!(
                    "node {} read as {} lacks declared label '{}'",
                    node.id, model.name, model.label
                ));
            }
            let declared_fields = declared_field_names(&model.fields);
            validate_props(
                "node",
                &model.name,
                node.id,
                &node.props,
                &model.fields,
                &declared_fields,
                report,
            );
        }
    }

    for (source_id, models) in node_seen {
        if models.len() > 1 {
            report.errors.push(format!(
                "source node {source_id} matched multiple declared node models: {}",
                models.join(", ")
            ));
        }
    }

    for (model_name, edges) in source_edges {
        let Some(model) = rel_by_model.get(model_name.as_str()) else {
            report
                .errors
                .push(format!("source edges for undeclared model '{model_name}'"));
            continue;
        };
        report.edge_counts.insert(model_name.clone(), edges.len());
        for edge in edges {
            for (endpoint, expected_model) in
                [(edge.from, &model.from_model), (edge.to, &model.to_model)]
            {
                if !ids_by_model
                    .get(expected_model.as_str())
                    .is_some_and(|ids| ids.contains(&endpoint))
                {
                    report.errors.push(format!(
                        "edge {}:{} endpoint {} does not match model '{}'",
                        model.name, edge.id, endpoint, expected_model
                    ));
                }
            }
            if edge.rel_type != model.rel_type {
                report.errors.push(format!(
                    "edge {} read as {} has type '{}', expected '{}'",
                    edge.id, model.name, edge.rel_type, model.rel_type
                ));
            }
            if !mappable_node_ids.contains(&edge.from) {
                report.errors.push(format!(
                    "edge {}:{} references unmapped from node {}",
                    model.name, edge.id, edge.from
                ));
            }
            if !mappable_node_ids.contains(&edge.to) {
                report.errors.push(format!(
                    "edge {}:{} references unmapped to node {}",
                    model.name, edge.id, edge.to
                ));
            }
            let declared_fields = declared_field_names(&model.fields);
            validate_props(
                "edge",
                &model.name,
                edge.id,
                &edge.props,
                &model.fields,
                &declared_fields,
                report,
            );
        }
    }

    report.total_source_nodes = report.node_counts.values().sum();
    report.total_source_edges = report.edge_counts.values().sum();
}

fn declared_field_names(fields: &[RuntimeField]) -> BTreeSet<&str> {
    fields.iter().map(|field| field.name.as_str()).collect()
}

fn validate_props(
    item_kind: &str,
    model: &str,
    id: i64,
    props: &BTreeMap<String, Value>,
    fields: &[RuntimeField],
    declared_fields: &BTreeSet<&str>,
    report: &mut MigrationReport,
) {
    for field in fields.iter().filter(|field| field.required) {
        if !props.contains_key(&field.name) {
            report.errors.push(format!(
                "{item_kind} {model}:{id} is missing required property '{}'",
                field.name
            ));
        }
    }
    for (name, value) in props {
        if !declared_fields.contains(name.as_str()) {
            report.errors.push(format!(
                "{item_kind} {model}:{id} has undeclared property '{name}'"
            ));
            continue;
        }
        let field = fields.iter().find(|field| field.name == *name).unwrap();
        if !grm_rs::validate_value_for_kind(field.value_type.into(), value) {
            report.errors.push(format!(
                "{item_kind} {model}:{id} property '{name}' does not match its declared type"
            ));
        }
        if let Err(err) = proto_property_value(value.clone()) {
            report.errors.push(format!(
                "{item_kind} {model}:{id} property '{name}' cannot be represented in service proto: {err}"
            ));
        }
    }
}

fn write_report_if_requested(
    options: &Options,
    report: &MigrationReport,
    id_map: Option<&MigrationIdMap>,
) -> Result<(), Box<dyn Error>> {
    if let Some(path) = &options.report {
        let mut report = report.clone();
        report.id_map_path = options
            .id_map
            .as_ref()
            .map(|path| path.display().to_string());
        report.id_map_entries = id_map.map(|map| IdMapEntryCounts {
            nodes: map.nodes.len(),
            edges: map.edges.len(),
        });
        write_json(path, &report)?;
    }
    Ok(())
}

fn write_id_map_if_requested(
    options: &Options,
    id_map: &MigrationIdMap,
) -> Result<(), Box<dyn Error>> {
    if let Some(path) = &options.id_map {
        write_json(path, id_map)?;
    }
    Ok(())
}

fn write_json(path: &PathBuf, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.set_len(0)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct MigrationReport {
    selection_path: Option<String>,
    execution: &'static str,
    source_uri_env: &'static str,
    source_database_env: Option<String>,
    schema: String,
    endpoint: String,
    workspace: String,
    workspace_mode: &'static str,
    workspace_format: &'static str,
    node_model_count: usize,
    edge_model_count: usize,
    node_counts: BTreeMap<String, usize>,
    edge_counts: BTreeMap<String, usize>,
    total_source_nodes: usize,
    total_source_edges: usize,
    total_target_nodes: Option<usize>,
    total_target_edges: Option<usize>,
    errors: Vec<String>,
    warnings: Vec<String>,
    id_map_path: Option<String>,
    id_map_entries: Option<IdMapEntryCounts>,
}

impl MigrationReport {
    fn new(
        options: &Options,
        node_models: &[RuntimeNodeModel],
        rel_models: &[RuntimeRelModel],
    ) -> Self {
        Self {
            selection_path: options
                .selection
                .as_ref()
                .map(|path| path.display().to_string()),
            execution: match options.execution {
                ExecutionMode::DryRun => "dry_run",
                ExecutionMode::Execute => "execute",
            },
            source_uri_env: "NEO4J_URI",
            source_database_env: None,
            schema: options.schema.display().to_string(),
            endpoint: options.endpoint.clone(),
            workspace: options.workspace.clone(),
            workspace_mode: match options.mode {
                WorkspaceMode::Create => "create",
                WorkspaceMode::Open => "open",
            },
            workspace_format: match options.format {
                proto::DurabilityFormat::Json => "json",
                proto::DurabilityFormat::Binary => "binary",
            },
            node_model_count: node_models.len(),
            edge_model_count: rel_models.len(),
            node_counts: BTreeMap::new(),
            edge_counts: BTreeMap::new(),
            total_source_nodes: 0,
            total_source_edges: 0,
            total_target_nodes: None,
            total_target_edges: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            id_map_path: None,
            id_map_entries: None,
        }
    }

    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
struct IdMapEntryCounts {
    nodes: usize,
    edges: usize,
}

#[derive(Debug, Serialize)]
struct MigrationIdMap {
    source_uri_env: &'static str,
    source_database_env: Option<String>,
    target_endpoint: String,
    target_workspace: String,
    nodes: Vec<NodeIdMapping>,
    edges: Vec<EdgeIdMapping>,
}

impl MigrationIdMap {
    fn new(options: &Options) -> Self {
        Self {
            source_uri_env: "NEO4J_URI",
            source_database_env: None,
            target_endpoint: options.endpoint.clone(),
            target_workspace: options.workspace.clone(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct NodeIdMapping {
    model: String,
    source_id: i64,
    target_id: i64,
}

#[derive(Debug, Serialize)]
struct EdgeIdMapping {
    model: String,
    source_id: i64,
    target_from: i64,
    target_to: i64,
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
            _ => Err(format!(
                "Neo4j query for '{}' returned a non-node result",
                model.name
            )
            .into()),
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
            _ => Err(format!(
                "Neo4j query for '{}' returned a non-edge result",
                model.name
            )
            .into()),
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
        _ => Err("expected create node response".into()),
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
        RuntimeValueType::Bytes => proto::FieldValueType::Bytes,
        RuntimeValueType::Decimal => proto::FieldValueType::Decimal,
        RuntimeValueType::Date => proto::FieldValueType::Date,
        RuntimeValueType::DateTime => proto::FieldValueType::Datetime,
        RuntimeValueType::Duration => proto::FieldValueType::Duration,
        RuntimeValueType::Uuid => proto::FieldValueType::Uuid,
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
        Value::Object(_) if grm_rs::typed_value_kind(&value) == Some("bytes") => {
            let raw =
                grm_rs::typed_value_payload(&value).ok_or("invalid typed bytes property value")?;
            Ok(proto::property_value::Kind::BytesValue(
                base64::engine::general_purpose::STANDARD.decode(raw)?,
            ))
        }
        Value::Object(_) if grm_rs::typed_value_kind(&value) == Some("decimal") => {
            Ok(proto::property_value::Kind::DecimalValue(
                grm_rs::typed_value_payload(&value)
                    .unwrap_or_default()
                    .into(),
            ))
        }
        Value::Object(_) if grm_rs::typed_value_kind(&value) == Some("date") => {
            Ok(proto::property_value::Kind::DateValue(
                grm_rs::typed_value_payload(&value)
                    .unwrap_or_default()
                    .into(),
            ))
        }
        Value::Object(_) if grm_rs::typed_value_kind(&value) == Some("datetime") => {
            Ok(proto::property_value::Kind::DatetimeValue(
                grm_rs::typed_value_payload(&value)
                    .unwrap_or_default()
                    .into(),
            ))
        }
        Value::Object(_) if grm_rs::typed_value_kind(&value) == Some("duration") => {
            Ok(proto::property_value::Kind::DurationValue(
                grm_rs::typed_value_payload(&value)
                    .unwrap_or_default()
                    .into(),
            ))
        }
        Value::Object(_) if grm_rs::typed_value_kind(&value) == Some("uuid") => {
            Ok(proto::property_value::Kind::UuidValue(
                grm_rs::typed_value_payload(&value)
                    .unwrap_or_default()
                    .into(),
            ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use grm_rs::BackendIdType;
    use serde_json::json;

    #[test]
    fn preflight_rejects_wrong_property_types_and_endpoint_models() {
        let models = vec![
            node_model(
                "Person",
                "personId",
                [field("name", RuntimeValueType::String)],
            ),
            node_model("Team", "teamId", [field("name", RuntimeValueType::String)]),
        ];
        let rels = vec![
            RuntimeRelModel::new(
                "Member",
                "Person",
                "Team",
                "memberId",
                BackendIdType::Int64,
                Vec::new(),
            )
            .unwrap(),
        ];
        let nodes = BTreeMap::from([
            (
                "Person".into(),
                vec![StoredNode {
                    id: 1,
                    labels: vec!["Person".into()],
                    props: props([("name", json!(42))]),
                }],
            ),
            (
                "Team".into(),
                vec![StoredNode {
                    id: 2,
                    labels: vec!["Team".into()],
                    props: props([("name", json!("Example"))]),
                }],
            ),
        ]);
        let edges = BTreeMap::from([(
            "Member".into(),
            vec![StoredRel {
                id: 3,
                from: 2,
                to: 1,
                rel_type: "Member".into(),
                props: BTreeMap::new(),
            }],
        )]);
        let mut report = report();
        preflight_source(&models, &rels, &nodes, &edges, &mut report);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("declared type"))
        );
        assert_eq!(
            report
                .errors
                .iter()
                .filter(|error| error.contains("does not match model"))
                .count(),
            2
        );
    }

    #[test]
    #[cfg(unix)]
    fn report_files_are_private_and_reject_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        fs::write(&path, "old report").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_json(&path, &json!({"count": 1})).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = dir.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert!(write_json(&link, &json!({"count": 2})).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap(),
            json!({"count": 1})
        );
    }

    #[test]
    fn preflight_rejects_duplicate_multilabel_source_nodes() {
        let node_models = vec![
            node_model(
                "Person",
                "personId",
                [field("name", RuntimeValueType::String)],
            ),
            node_model(
                "Agent",
                "agentId",
                [field("name", RuntimeValueType::String)],
            ),
        ];
        let rel_models = Vec::new();
        let mut source_nodes = BTreeMap::new();
        source_nodes.insert(
            "Person".into(),
            vec![StoredNode {
                id: 7,
                labels: vec!["Person".into(), "Agent".into()],
                props: props([("name", json!("Ada"))]),
            }],
        );
        source_nodes.insert(
            "Agent".into(),
            vec![StoredNode {
                id: 7,
                labels: vec!["Person".into(), "Agent".into()],
                props: props([("name", json!("Ada"))]),
            }],
        );
        let source_edges = BTreeMap::new();
        let mut report = report();

        preflight_source(
            &node_models,
            &rel_models,
            &source_nodes,
            &source_edges,
            &mut report,
        );

        assert!(report.has_errors());
        assert!(report.errors.iter().any(|error| {
            error.contains("source node 7 matched multiple declared node models")
        }));
    }

    #[test]
    fn preflight_accepts_typed_values_and_counts_empty_models() {
        let node_models = vec![
            node_model(
                "Event",
                "eventId",
                [
                    field("event_id", RuntimeValueType::Uuid),
                    field("payload", RuntimeValueType::Bytes),
                ],
            ),
            node_model(
                "Empty",
                "emptyId",
                [field("name", RuntimeValueType::String)],
            ),
        ];
        let rel_models = vec![
            RuntimeRelModel::new(
                "Follows",
                "Event",
                "Event",
                "followsId",
                BackendIdType::Int64,
                Vec::new(),
            )
            .unwrap(),
        ];
        let mut source_nodes = BTreeMap::new();
        source_nodes.insert(
            "Event".into(),
            vec![StoredNode {
                id: 1,
                labels: vec!["Event".into()],
                props: props([
                    (
                        "event_id",
                        json!({"$grm_type": "uuid", "value": "123e4567-e89b-12d3-a456-426614174000"}),
                    ),
                    ("payload", json!({"$grm_type": "bytes", "value": "AQID"})),
                ]),
            }],
        );
        source_nodes.insert("Empty".into(), Vec::new());
        let mut source_edges = BTreeMap::new();
        source_edges.insert("Follows".into(), Vec::new());
        let mut report = report();

        preflight_source(
            &node_models,
            &rel_models,
            &source_nodes,
            &source_edges,
            &mut report,
        );

        assert!(!report.has_errors(), "{:?}", report.errors);
        assert_eq!(report.node_counts["Event"], 1);
        assert_eq!(report.node_counts["Empty"], 0);
        assert_eq!(report.edge_counts["Follows"], 0);
        assert_eq!(report.total_source_nodes, 1);
        assert_eq!(report.total_source_edges, 0);
    }

    #[test]
    fn selection_preserves_only_explicit_records_and_rejects_dangling_edges() {
        let nodes = BTreeMap::from([(
            "Context".into(),
            vec![
                StoredNode {
                    id: 1,
                    labels: vec!["Context".into()],
                    props: BTreeMap::new(),
                },
                StoredNode {
                    id: 2,
                    labels: vec!["Context".into()],
                    props: BTreeMap::new(),
                },
                StoredNode {
                    id: 3,
                    labels: vec!["Context".into()],
                    props: BTreeMap::new(),
                },
            ],
        )]);
        let edges = BTreeMap::from([(
            "Link".into(),
            vec![StoredRel {
                id: 4,
                from: 1,
                to: 2,
                rel_type: "Link".into(),
                props: BTreeMap::new(),
            }],
        )]);
        for invalid in [
            MigrationSelection {
                node_ids: vec![1],
                edge_ids: vec![4],
            },
            MigrationSelection {
                node_ids: vec![1, 99],
                edge_ids: vec![],
            },
            MigrationSelection {
                node_ids: vec![1, 1],
                edge_ids: vec![],
            },
            MigrationSelection {
                node_ids: vec![],
                edge_ids: vec![],
            },
        ] {
            let mut selected_nodes = nodes.clone();
            let mut selected_edges = edges.clone();
            assert!(select_source(&invalid, &mut selected_nodes, &mut selected_edges).is_err());
            assert_eq!(selected_nodes["Context"].len(), 3);
            assert_eq!(selected_edges["Link"].len(), 1);
        }
        let mut selected_nodes = nodes;
        let mut selected_edges = edges;
        select_source(
            &MigrationSelection {
                node_ids: vec![1, 2],
                edge_ids: vec![4],
            },
            &mut selected_nodes,
            &mut selected_edges,
        )
        .unwrap();
        assert_eq!(
            selected_nodes["Context"]
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(selected_edges["Link"].len(), 1);
    }

    fn report() -> MigrationReport {
        let options = Options {
            schema: PathBuf::from("schema.json"),
            endpoint: "http://127.0.0.1:50051".into(),
            workspace: "migration-test".into(),
            mode: WorkspaceMode::Create,
            format: proto::DurabilityFormat::Binary,
            execution: ExecutionMode::DryRun,
            allow_open_target: false,
            report: None,
            id_map: None,
            selection: None,
        };
        MigrationReport::new(&options, &[], &[])
    }

    fn node_model<const N: usize>(
        name: &str,
        id_field: &str,
        fields: [RuntimeField; N],
    ) -> RuntimeNodeModel {
        RuntimeNodeModel::new(
            name,
            id_field,
            BackendIdType::Int64,
            fields.into_iter().collect(),
        )
        .unwrap()
    }

    fn field(name: &str, value_type: RuntimeValueType) -> RuntimeField {
        RuntimeField {
            name: name.into(),
            value_type,
            required: true,
        }
    }

    fn props<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
        entries
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect()
    }
}
