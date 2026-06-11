use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;

use grm_rs::runtime::{KeyValueArg, SessionCommand, parse_command_line};
use grm_rs::{
    CliSession, DefineEdgeRequest, DefineNodeRequest, EdgeCreateRequest, EdgeDeleteRequest,
    EdgeFindRequest, EdgeUpdateRequest, ExplainRequest, FieldSpec, FieldValueType,
    NodeCreateRequest, NodeDeleteRequest, NodeFindRequest, NodeUpdateRequest, ProfileRequest,
    QueryRequest, QueryTerm, RuntimeNodeModel, RuntimeRelModel, StoredNode, StoredRel,
};
use grm_service_api::{
    DurabilityFormat, GrpcClientTlsOptions, GrpcWorkspaceClient, GrpcWorkspaceMode, proto,
};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupLoadFormat {
    Json,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupAutocommit {
    Default,
    On,
    Off,
}

#[derive(Debug, PartialEq, Eq)]
enum SessionStartup {
    Fresh,
    Script {
        path: PathBuf,
    },
    Load {
        format: StartupLoadFormat,
        path: PathBuf,
        autocommit: StartupAutocommit,
    },
}

fn should_enable_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("session") => {
            let startup = match parse_session_startup(args.collect()) {
                Ok(startup) => startup,
                Err(message) => {
                    eprintln!("{message}");
                    eprintln!("{}", session_usage());
                    std::process::exit(1);
                }
            };
            let stdout = io::stdout();
            let writer = stdout.lock();
            if std::env::var("GRM_BACKEND").ok().as_deref() == Some("grpc") {
                let script = match startup {
                    SessionStartup::Fresh => None,
                    SessionStartup::Script { path } => Some(path),
                    SessionStartup::Load { .. } => {
                        eprintln!(
                            "--load is not supported in gRPC service mode; use GRM_SERVICE_WORKSPACE_MODE=open"
                        );
                        std::process::exit(1);
                    }
                };
                let stdin = io::stdin();
                let reader = BufReader::new(stdin.lock());
                if let Err(err) =
                    run_service_session(reader, writer, should_enable_color(), script).await
                {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
                return;
            }
            match startup {
                SessionStartup::Script { path } => {
                    let file = match File::open(&path) {
                        Ok(file) => file,
                        Err(err) => {
                            eprintln!("failed to open script '{}': {err}", path.display());
                            std::process::exit(1);
                        }
                    };
                    let reader = BufReader::new(file);
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());
                    if let Err(err) = session.run_script().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }

                    let (state, _, writer) = session.into_parts();
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session = CliSession::with_state_and_color(
                        state,
                        reader,
                        writer,
                        should_enable_color(),
                    );
                    if let Err(err) = session.continue_interactive().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
                SessionStartup::Load {
                    format,
                    path,
                    autocommit,
                } => {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());

                    let load_result = match format {
                        StartupLoadFormat::Json => session.load_session_json(&path),
                        StartupLoadFormat::Binary => session.load_session_binary(&path),
                    };
                    if let Err(err) = load_result {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }

                    let autocommit_result = match (format, autocommit) {
                        (_, StartupAutocommit::Default | StartupAutocommit::Off) => {
                            session.write_startup_autocommit_off()
                        }
                        (StartupLoadFormat::Json, StartupAutocommit::On) => session
                            .enable_autocommit_json(&path)
                            .and_then(|_| session.write_startup_autocommit_on(&path)),
                        (StartupLoadFormat::Binary, StartupAutocommit::On) => session
                            .enable_autocommit_binary(&path)
                            .and_then(|_| session.write_startup_autocommit_on(&path)),
                    };
                    if let Err(err) = autocommit_result {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }

                    if let Err(err) = session.continue_loaded_interactive().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
                SessionStartup::Fresh => {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    let mut session =
                        CliSession::new_with_color(reader, writer, should_enable_color());
                    if let Err(err) = session.run().await {
                        eprintln!("{err}");
                        std::process::exit(1);
                    }
                }
            }
        }
        _ => {
            eprintln!("{}", session_usage());
            std::process::exit(1);
        }
    }
}

fn parse_session_startup(args: Vec<String>) -> Result<SessionStartup, String> {
    if args.is_empty() {
        return Ok(SessionStartup::Fresh);
    }

    let mut script = None;
    let mut load = None;
    let mut autocommit = StartupAutocommit::Default;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--script" => {
                let Some(path) = args.get(index + 1) else {
                    return Err("--script requires <path>".to_string());
                };
                script = Some(PathBuf::from(path));
                index += 2;
            }
            "--load" => {
                let Some(format) = args.get(index + 1) else {
                    return Err("--load requires json|bin and <path>".to_string());
                };
                let Some(path) = args.get(index + 2) else {
                    return Err("--load requires json|bin and <path>".to_string());
                };
                let format = match format.as_str() {
                    "json" => StartupLoadFormat::Json,
                    "bin" => StartupLoadFormat::Binary,
                    other => {
                        return Err(format!("unknown --load format '{other}'"));
                    }
                };
                load = Some((format, PathBuf::from(path)));
                index += 3;
            }
            "--autocommit" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--autocommit requires on|off".to_string());
                };
                autocommit = match value.as_str() {
                    "on" => StartupAutocommit::On,
                    "off" => StartupAutocommit::Off,
                    other => {
                        return Err(format!("unknown --autocommit value '{other}'"));
                    }
                };
                index += 2;
            }
            other => {
                return Err(format!("unknown session option '{other}'"));
            }
        }
    }

    if script.is_some() && load.is_some() {
        return Err("--script and --load cannot be combined yet".to_string());
    }

    if let Some(path) = script {
        if autocommit != StartupAutocommit::Default {
            return Err("--autocommit requires --load".to_string());
        }
        return Ok(SessionStartup::Script { path });
    }

    if let Some((format, path)) = load {
        return Ok(SessionStartup::Load {
            format,
            path,
            autocommit,
        });
    }

    if autocommit != StartupAutocommit::Default {
        return Err("--autocommit requires --load".to_string());
    }

    Ok(SessionStartup::Fresh)
}

fn session_usage() -> &'static str {
    concat!(
        "Usage: grm session [--script <path> | --load json|bin <path> ",
        "[--autocommit on|off]]\n\n",
        "Local mode:\n  cargo run --bin grm -- session\n\n",
        "Service-backed workspace mode:\n",
        "  GRM_BACKEND=grpc GRM_SERVICE_ENDPOINT=<url> GRM_WORKSPACE_REF=<ref> ",
        "\\\n",
        "    GRM_SERVICE_WORKSPACE_MODE=create|open cargo run --bin grm -- session ",
        "[--script <path>]\n\n",
        "Set GRM_SERVICE_TLS_CA_CERT and GRM_SERVICE_TLS_DOMAIN_NAME for ",
        "server-authenticated TLS.\n",
        "Set GRM_SERVICE_TLS_CLIENT_CERT and GRM_SERVICE_TLS_CLIENT_KEY when ",
        "the service requires mutual TLS.\n",
        "GRM_SERVICE_WORKSPACE_MODE defaults to open when omitted.\n",
        "GRM_SERVICE_WORKSPACE_FORMAT defaults to binary; set json only as ",
        "an explicit opt-in.\n",
        "Service scripts are parsed by the CLI and supported commands route ",
        "through ExecuteWorkspace.\n",
        "--load remains local-only; reopen a service workspace with ",
        "GRM_SERVICE_WORKSPACE_MODE=open."
    )
}

async fn run_service_session<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    color_enabled: bool,
    script: Option<PathBuf>,
) -> grm_rs::Result<()> {
    let endpoint = required_env("GRM_SERVICE_ENDPOINT")?;
    let workspace_ref = required_env("GRM_WORKSPACE_REF")?;
    let mode = match std::env::var("GRM_SERVICE_WORKSPACE_MODE").ok().as_deref() {
        Some("create") => GrpcWorkspaceMode::Create,
        Some("open") | None => GrpcWorkspaceMode::Open,
        Some(other) => {
            return Err(grm_rs::GrmError::Constraint(format!(
                "unsupported GRM_SERVICE_WORKSPACE_MODE '{other}'; expected 'create' or 'open'"
            )));
        }
    };
    let format_env = std::env::var("GRM_SERVICE_WORKSPACE_FORMAT").ok();
    let format = match format_env.as_deref() {
        Some("json") => DurabilityFormat::Json,
        Some("bin" | "binary") | None => DurabilityFormat::Binary,
        Some(other) => {
            return Err(grm_rs::GrmError::Constraint(format!(
                "unsupported GRM_SERVICE_WORKSPACE_FORMAT '{other}'; expected 'json', 'bin', or 'binary'"
            )));
        }
    };
    let tls = GrpcClientTlsOptions::from_env().map_err(grm_rs::GrmError::from)?;
    let tls_enabled = tls.is_some();
    let client_certificate_configured =
        tls.as_ref().is_some_and(GrpcClientTlsOptions::has_identity);
    let mut client = GrpcWorkspaceClient::connect_with_format_and_tls(
        &endpoint,
        &workspace_ref,
        mode,
        format,
        tls,
    )
    .await
    .map_err(service_error)?;

    writeln!(
        writer,
        "Welcome to GRM-RS CLI.\nService-backed workspace session ready."
    )?;
    writeln!(writer, "Backend: gRPC workspace storage")?;
    writeln!(writer, "Endpoint: {endpoint}")?;
    writeln!(
        writer,
        "Transport: {}",
        if client_certificate_configured {
            "TLS with client certificate"
        } else if tls_enabled {
            "TLS"
        } else {
            "insecure local gRPC"
        }
    )?;
    writeln!(writer, "Workspace: {workspace_ref}")?;
    writeln!(writer, "Mode: {}", service_mode_display(mode))?;
    writeln!(
        writer,
        "Persistence format: {}{}",
        durability_format_display(format),
        if format_env.is_some() {
            " (explicit)"
        } else {
            " (default)"
        }
    )?;
    writeln!(
        writer,
        "Scope: ExecuteWorkspace. Unsupported commands stay local-only or unavailable in this mode."
    )?;
    let mut session = ServiceCliSession::new(
        &mut client,
        ServiceSessionInfo {
            endpoint,
            mode,
            format,
            format_explicit: format_env.is_some(),
        },
        color_enabled,
    );
    if let Some(path) = script {
        let file = File::open(&path).map_err(|error| {
            grm_rs::GrmError::Backend(format!(
                "failed to open service script '{}': {error}",
                path.display()
            ))
        })?;
        writeln!(writer, "Running service setup script: {}", path.display())?;
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            let line = strip_service_script_comment(&line);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match session.handle_command(&mut writer, trimmed, true).await {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    return Err(grm_rs::GrmError::Constraint(format!(
                        "service script '{}' line {} failed: {error}",
                        path.display(),
                        index + 1
                    )));
                }
            }
        }
        writeln!(writer, "Service setup script complete.")?;
    }
    let mut lines = reader.lines();
    loop {
        write!(writer, "grm(service)> ")?;
        writer.flush()?;
        let Some(line) = lines.next() else {
            writeln!(writer)?;
            break;
        };
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match session.handle_command(&mut writer, trimmed, false).await {
            Ok(true) => break,
            Ok(false) => {}
            Err(err) => writeln!(writer, "{err}")?,
        }
    }
    Ok(())
}

struct ServiceCliSession<'a> {
    client: &'a mut GrpcWorkspaceClient,
    info: ServiceSessionInfo,
    bindings: BTreeMap<String, i64>,
    colors: CliColors,
}

#[derive(Debug, Clone)]
struct ServiceSessionInfo {
    endpoint: String,
    mode: GrpcWorkspaceMode,
    format: DurabilityFormat,
    format_explicit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceOutputFormat {
    Default,
    Jsonl,
    Table,
    Graph,
}

impl<'a> ServiceCliSession<'a> {
    fn new(
        client: &'a mut GrpcWorkspaceClient,
        info: ServiceSessionInfo,
        color_enabled: bool,
    ) -> ServiceCliSession<'a> {
        ServiceCliSession {
            client,
            info,
            bindings: BTreeMap::new(),
            colors: CliColors::for_terminal(color_enabled),
        }
    }

    async fn handle_command<W: Write>(
        &mut self,
        writer: &mut W,
        line: &str,
        fail_fast: bool,
    ) -> grm_rs::Result<bool> {
        match parse_command_line(line)? {
            SessionCommand::Help => write_service_help(writer, &self.colors)?,
            SessionCommand::Exit => return Ok(true),
            SessionCommand::SessionDescribe { verbose } => {
                self.write_summary(writer, verbose).await?;
            }
            SessionCommand::ModelList | SessionCommand::LinkList => {
                let schema = self.client.schema_list().await.map_err(service_error)?;
                write_service_schema(
                    writer,
                    &schema.node_models,
                    &schema.edge_models,
                    &self.colors,
                )?;
            }
            SessionCommand::ModelDefine { args } => {
                let request = parse_model_define_args(args)?;
                self.client
                    .define_node(request)
                    .await
                    .map_err(service_error)?;
                writeln!(writer, "Defined node model")?;
            }
            SessionCommand::LinkDefine { args } => {
                let request = parse_link_define_args(args)?;
                self.client
                    .define_edge(request)
                    .await
                    .map_err(service_error)?;
                writeln!(writer, "Defined edge model")?;
            }
            SessionCommand::NodeCreate {
                binding,
                model_name,
                assignments,
            } => {
                if let Some(binding) = &binding {
                    ensure_binding_available(&self.bindings, binding)?;
                }
                let node = self
                    .client
                    .create_node(NodeCreateRequest {
                        model: model_name,
                        props: assignments_to_json(assignments, &self.bindings)?,
                    })
                    .await
                    .map_err(service_error)?;
                if let Some(binding) = binding {
                    self.bindings.insert(binding, node.id);
                }
                write_node(writer, &node, &self.colors)?;
            }
            SessionCommand::NodeFind { model_name, terms } => {
                let format = service_output_format(&terms)?;
                if format == ServiceOutputFormat::Graph {
                    return Err(grm_rs::GrmError::NotSupported(
                        "format=graph is not supported in gRPC CLI mode because the service response does not currently include traversal path rows",
                    ));
                }
                let request = NodeFindRequest::from_adapter_query_terms(model_name, terms)?;
                let found = self
                    .client
                    .find_node_results(request)
                    .await
                    .map_err(service_error)?;
                write_service_find_results(
                    writer,
                    &found.nodes,
                    &found.edges,
                    format,
                    &self.colors,
                )?;
            }
            SessionCommand::NodeUpdate {
                model_name,
                id,
                assignments,
            } => {
                let node = self
                    .client
                    .update_node(NodeUpdateRequest {
                        model: model_name,
                        id: parse_i64_or_binding(&id, "node id", &self.bindings)?,
                        props: assignments_to_json(assignments, &self.bindings)?,
                    })
                    .await
                    .map_err(service_error)?;
                write_node(writer, &node, &self.colors)?;
            }
            SessionCommand::NodeDelete { model_name, id } => {
                let deleted = self
                    .client
                    .delete_node(NodeDeleteRequest {
                        model: model_name,
                        id: parse_i64_or_binding(&id, "node id", &self.bindings)?,
                    })
                    .await
                    .map_err(service_error)?;
                writeln!(writer, "Deleted node {} id={}", deleted.model, deleted.id)?;
            }
            SessionCommand::EdgeCreate {
                model_name,
                assignments,
            } => {
                let mut props = assignments_to_json(assignments, &self.bindings)?;
                let from = take_required_id_or_binding(&mut props, "from", &self.bindings)?;
                let to = take_required_id_or_binding(&mut props, "to", &self.bindings)?;
                let edge = self
                    .client
                    .create_edge(EdgeCreateRequest {
                        model: model_name,
                        from,
                        to,
                        props,
                    })
                    .await
                    .map_err(service_error)?;
                write_edge(writer, &edge, &self.colors)?;
            }
            SessionCommand::EdgeFind { model_name, terms } => {
                let format = service_output_format(&terms)?;
                if format == ServiceOutputFormat::Graph {
                    return Err(grm_rs::GrmError::NotSupported(
                        "format=graph is only supported for graph-shaped node.find results and is not available in gRPC CLI mode",
                    ));
                }
                let request = EdgeFindRequest::from_adapter_filter_values(
                    model_name,
                    terms_to_json_filters(terms, &self.bindings, true)?,
                )?;
                let found = self
                    .client
                    .find_edges(request)
                    .await
                    .map_err(service_error)?;
                write_service_find_results(writer, &[], &found.edges, format, &self.colors)?;
            }
            SessionCommand::EdgeUpdate {
                model_name,
                id,
                assignments,
            } => {
                let edge = self
                    .client
                    .update_edge(EdgeUpdateRequest {
                        model: model_name,
                        id: parse_i64_or_binding(&id, "edge id", &self.bindings)?,
                        props: assignments_to_json(assignments, &self.bindings)?,
                    })
                    .await
                    .map_err(service_error)?;
                write_edge(writer, &edge, &self.colors)?;
            }
            SessionCommand::EdgeDelete { model_name, id } => {
                let deleted = self
                    .client
                    .delete_edge(EdgeDeleteRequest {
                        model: model_name,
                        id: parse_i64_or_binding(&id, "edge id", &self.bindings)?,
                    })
                    .await
                    .map_err(service_error)?;
                writeln!(writer, "Deleted edge {} id={}", deleted.model, deleted.id)?;
            }
            SessionCommand::SessionExplainNodeFind {
                model_name,
                terms,
                verbose: _,
            } => {
                let request = NodeFindRequest::from_adapter_query_terms(model_name, terms)?;
                let explain = self
                    .client
                    .explain(ExplainRequest {
                        query: QueryRequest::NodeFind(request),
                    })
                    .await
                    .map_err(service_error)?;
                write_service_explain(writer, &explain)?;
            }
            SessionCommand::SessionProfileNodeFind {
                model_name,
                terms,
                verbose: _,
            } => {
                let request = NodeFindRequest::from_adapter_query_terms(model_name, terms)?;
                let profile = self
                    .client
                    .profile(ProfileRequest {
                        query: QueryRequest::NodeFind(request),
                    })
                    .await
                    .map_err(service_error)?;
                write_service_profile(writer, &profile)?;
            }
            SessionCommand::SessionExplainEdgeFind {
                model_name,
                terms,
                verbose: _,
            } => {
                let request = EdgeFindRequest::from_adapter_filter_values(
                    model_name,
                    terms_to_json_filters(terms, &self.bindings, true)?,
                )?;
                let explain = self
                    .client
                    .explain(ExplainRequest {
                        query: QueryRequest::EdgeFind(request),
                    })
                    .await
                    .map_err(service_error)?;
                write_service_explain(writer, &explain)?;
            }
            SessionCommand::SessionProfileEdgeFind {
                model_name,
                terms,
                verbose: _,
            } => {
                let request = EdgeFindRequest::from_adapter_filter_values(
                    model_name,
                    terms_to_json_filters(terms, &self.bindings, true)?,
                )?;
                let profile = self
                    .client
                    .profile(ProfileRequest {
                        query: QueryRequest::EdgeFind(request),
                    })
                    .await
                    .map_err(service_error)?;
                write_service_profile(writer, &profile)?;
            }
            SessionCommand::Unknown { .. } if fail_fast => {
                return Err(grm_rs::GrmError::Constraint(format!(
                    "unknown command: {line}"
                )));
            }
            SessionCommand::Unknown { .. } => writeln!(writer, "Unknown command: {line}")?,
            SessionCommand::SessionSave { args: _ }
            | SessionCommand::SessionLoad { args: _ }
            | SessionCommand::SessionImport { args: _ }
            | SessionCommand::SessionExport { args: _ }
            | SessionCommand::SessionCompact
            | SessionCommand::SessionAutocommit { args: _ }
            | SessionCommand::SessionIndexes { .. }
            | SessionCommand::TxBegin
            | SessionCommand::TxCommit
            | SessionCommand::ModelShow { .. }
            | SessionCommand::LinkShow { .. } => {
                let message = "command is local-only or not supported in gRPC service CLI mode yet";
                if fail_fast {
                    return Err(grm_rs::GrmError::NotSupported(message));
                }
                writeln!(
                    writer,
                    "Command is local-only or not supported in gRPC service CLI mode yet"
                )?;
            }
        }
        Ok(false)
    }

    async fn write_summary<W: Write>(
        &mut self,
        writer: &mut W,
        verbose: bool,
    ) -> grm_rs::Result<()> {
        let schema = self.client.schema_list().await.map_err(service_error)?;
        writeln!(writer, "Session Summary")?;
        writeln!(writer, "Backend: gRPC workspace storage")?;
        writeln!(writer, "Endpoint: {}", self.info.endpoint)?;
        writeln!(writer, "Workspace: {}", self.client.workspace_ref().id)?;
        writeln!(writer, "Mode: {}", service_mode_display(self.info.mode))?;
        writeln!(
            writer,
            "Persistence format: {}{}",
            durability_format_display(self.info.format),
            if self.info.format_explicit {
                " (explicit)"
            } else {
                " (default)"
            }
        )?;
        writeln!(writer, "Scope: ExecuteWorkspace")?;

        writeln!(writer, "Types defined:")?;
        if schema.node_models.is_empty() && schema.edge_models.is_empty() {
            writeln!(writer, "  none")?;
        } else {
            if !schema.node_models.is_empty() {
                let nodes = schema
                    .node_models
                    .iter()
                    .map(|model| self.colors.type_name(&model.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(writer, "  nodes: {nodes}")?;
            }
            if !schema.edge_models.is_empty() {
                let edges = schema
                    .edge_models
                    .iter()
                    .map(|model| self.colors.type_name(&model.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(writer, "  links: {edges}")?;
            }
        }

        let mut rows = Vec::new();
        let mut node_total = 0usize;
        for model in &schema.node_models {
            let found = self
                .client
                .find_nodes(NodeFindRequest {
                    model: model.name.clone(),
                    ..Default::default()
                })
                .await
                .map_err(service_error)?;
            node_total += found.nodes.len();
            if !found.nodes.is_empty() {
                rows.push(vec![
                    "node".to_string(),
                    self.colors.type_name(&model.name),
                    found.nodes.len().to_string(),
                ]);
            }
        }

        let mut edge_total = 0usize;
        for model in &schema.edge_models {
            let found = self
                .client
                .find_edges(EdgeFindRequest {
                    model: model.name.clone(),
                    ..Default::default()
                })
                .await
                .map_err(service_error)?;
            edge_total += found.edges.len();
            if !found.edges.is_empty() {
                rows.push(vec![
                    "edge".to_string(),
                    self.colors.type_name(&model.name),
                    found.edges.len().to_string(),
                ]);
            }
        }

        writeln!(
            writer,
            "Stored rows: {node_total} nodes, {edge_total} edges"
        )?;
        writeln!(writer, "By type:")?;
        if rows.is_empty() {
            writeln!(writer, "  none")?;
        } else {
            write_cli_table(
                writer,
                &["kind", "type", "count"],
                &[
                    TableHeaderKind::Plain,
                    TableHeaderKind::Type,
                    TableHeaderKind::Property,
                ],
                &rows,
                &self.colors,
            )?;
        }

        writeln!(writer, "Autocommit: service-managed local workspace")?;
        if verbose {
            writeln!(
                writer,
                "Supported traversal subset: node.find via/end/edge filters with return=root|end|edge, plus session.explain/profile for node.find and edge.find. Unsupported in gRPC CLI mode: local session save/load/import/export, transactions, free-form query parity, and session.indexes"
            )?;
        }
        Ok(())
    }
}

fn strip_service_script_comment(line: &str) -> String {
    let mut quote: Option<char> = None;
    let mut chars = line.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        match quote {
            Some(quote_char) => match ch {
                '\\' => {
                    chars.next();
                }
                _ if ch == quote_char => quote = None,
                _ => {}
            },
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '#' => return line[..index].trim_end().to_string(),
                _ => {}
            },
        }
    }

    line.to_string()
}

fn service_mode_display(mode: GrpcWorkspaceMode) -> &'static str {
    match mode {
        GrpcWorkspaceMode::Create => "create",
        GrpcWorkspaceMode::Open => "open",
    }
}

fn durability_format_display(format: DurabilityFormat) -> &'static str {
    match format {
        DurabilityFormat::Json => "json",
        DurabilityFormat::Binary => "binary",
    }
}

fn write_service_help<W: Write>(writer: &mut W, colors: &CliColors) -> grm_rs::Result<()> {
    writeln!(writer, "Available commands in gRPC service mode:")?;
    writeln!(
        writer,
        "  {} <Name> <id_field> [field:type:required|optional ...]",
        colors.property_name("model.define")
    )?;
    writeln!(
        writer,
        "  {} <Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]",
        colors.property_name("link.define")
    )?;
    writeln!(
        writer,
        "  {} | {} | {}",
        colors.property_name("model.list"),
        colors.property_name("link.list"),
        colors.property_name("session.describe")
    )?;
    writeln!(
        writer,
        "  {} <ModelName> [field=value ...]",
        colors.property_name("node.create")
    )?;
    writeln!(
        writer,
        "  let <name> = {} <ModelName> [field=value ...]",
        colors.property_name("node.create")
    )?;
    writeln!(
        writer,
        "  {} <ModelName> [field=value|field!=value|field>value|field>=value|field<value|field<=value|field~value ...] [via=<out|in|both>:<LinkName|*>:<EndModel> ...] [end.<field>=value ...] [edge.<field>=value ...] [return=root|end|edge] [order=<field>:asc|desc[,<field>:asc|desc ...]] [limit=<n>] [offset=<n>] [format=default|jsonl|table]",
        colors.property_name("node.find")
    )?;
    writeln!(
        writer,
        "  {} <ModelName> <id|binding> [field=value ...]",
        colors.property_name("node.update")
    )?;
    writeln!(
        writer,
        "  {} <ModelName> <id|binding>",
        colors.property_name("node.delete")
    )?;
    writeln!(
        writer,
        "  {} <LinkName> from=<id|binding> to=<id|binding> [field=value ...]",
        colors.property_name("edge.create")
    )?;
    writeln!(
        writer,
        "  {} <LinkName> [from=<id|binding>] [to=<id|binding>] [field=value|field!=value|field>value|field>=value|field<value|field<=value|field~value ...] [order=<field>:asc|desc[,<field>:asc|desc ...]] [limit=<n>] [offset=<n>] [format=default|jsonl|table]",
        colors.property_name("edge.find")
    )?;
    writeln!(
        writer,
        "  {} <LinkName> <id|binding> [field=value ...]",
        colors.property_name("edge.update")
    )?;
    writeln!(
        writer,
        "  {} <LinkName> <id|binding>",
        colors.property_name("edge.delete")
    )?;
    writeln!(
        writer,
        "  {} [--verbose]",
        colors.property_name("session.describe")
    )?;
    writeln!(writer, "  {}", colors.property_name("session.help"))?;
    writeln!(writer, "  {}", colors.property_name("session.exit"))?;
    writeln!(writer)?;
    writeln!(
        writer,
        "Local-only or unsupported in gRPC service mode: session.save/load/import/export, session.autocommit, session.compact, tx.begin/commit, free-form query parity, session.indexes, model.show, and link.show."
    )?;
    Ok(())
}

fn write_service_schema<W: Write>(
    writer: &mut W,
    nodes: &[RuntimeNodeModel],
    edges: &[RuntimeRelModel],
    colors: &CliColors,
) -> grm_rs::Result<()> {
    writeln!(writer, "Service Schema")?;
    let mut rows = Vec::new();
    for node in nodes {
        rows.push(vec![
            "node".to_string(),
            colors.type_name(&node.name),
            colors.property_name(&node.id_field_name),
            String::new(),
        ]);
    }
    for edge in edges {
        rows.push(vec![
            "edge".to_string(),
            colors.type_name(&edge.name),
            colors.property_name(&edge.id_field_name),
            format!(
                "{} -> {}",
                colors.type_name(&edge.from_model),
                colors.type_name(&edge.to_model)
            ),
        ]);
    }
    if rows.is_empty() {
        writeln!(writer, "  none")?;
    } else {
        write_cli_table(
            writer,
            &["kind", "type", "id field", "endpoints"],
            &[
                TableHeaderKind::Plain,
                TableHeaderKind::Type,
                TableHeaderKind::Property,
                TableHeaderKind::Plain,
            ],
            &rows,
            colors,
        )?;
    }
    Ok(())
}

fn write_node<W: Write>(
    writer: &mut W,
    node: &StoredNode,
    colors: &CliColors,
) -> grm_rs::Result<()> {
    writeln!(
        writer,
        "Node {} id={} {}",
        colors.type_name(node.labels.first().map(String::as_str).unwrap_or("")),
        node.id,
        props_display(&node.props, colors)
    )?;
    Ok(())
}

fn write_edge<W: Write>(
    writer: &mut W,
    edge: &StoredRel,
    colors: &CliColors,
) -> grm_rs::Result<()> {
    writeln!(
        writer,
        "Edge {} id={} from={} to={} {}",
        colors.type_name(&edge.rel_type),
        edge.id,
        edge.from,
        edge.to,
        props_display(&edge.props, colors)
    )?;
    Ok(())
}

fn service_output_format(terms: &[QueryTerm]) -> grm_rs::Result<ServiceOutputFormat> {
    let Some(raw) = terms
        .iter()
        .find(|term| term.key == "format")
        .map(|term| term.value.as_str())
    else {
        return Ok(ServiceOutputFormat::Default);
    };
    match raw {
        "default" => Ok(ServiceOutputFormat::Default),
        "jsonl" => Ok(ServiceOutputFormat::Jsonl),
        "table" => Ok(ServiceOutputFormat::Table),
        "graph" => Ok(ServiceOutputFormat::Graph),
        _ => Err(grm_rs::GrmError::Constraint(
            "format must be one of: default, jsonl, table, graph".into(),
        )),
    }
}

fn write_service_find_results<W: Write>(
    writer: &mut W,
    nodes: &[StoredNode],
    edges: &[StoredRel],
    format: ServiceOutputFormat,
    colors: &CliColors,
) -> grm_rs::Result<()> {
    match format {
        ServiceOutputFormat::Default => {
            for node in nodes {
                write_node(writer, node, colors)?;
            }
            for edge in edges {
                write_edge(writer, edge, colors)?;
            }
        }
        ServiceOutputFormat::Jsonl => {
            for node in nodes {
                writeln!(
                    writer,
                    "{}",
                    json!({
                        "kind": "node",
                        "model": node.labels.first().cloned().unwrap_or_default(),
                        "id": node.id,
                        "labels": node.labels,
                        "props": node.props,
                    })
                )?;
            }
            for edge in edges {
                writeln!(
                    writer,
                    "{}",
                    json!({
                        "kind": "edge",
                        "model": edge.rel_type,
                        "id": edge.id,
                        "from": edge.from,
                        "to": edge.to,
                        "type": edge.rel_type,
                        "props": edge.props,
                    })
                )?;
            }
        }
        ServiceOutputFormat::Table => {
            if !nodes.is_empty() {
                write_service_node_table(writer, nodes, colors)?;
            }
            if !edges.is_empty() {
                write_service_edge_table(writer, edges, colors)?;
            }
            if nodes.is_empty() && edges.is_empty() {
                writeln!(writer, "No results.")?;
            }
        }
        ServiceOutputFormat::Graph => {
            return Err(grm_rs::GrmError::NotSupported(
                "format=graph is not supported by flat service find responses",
            ));
        }
    }
    Ok(())
}

fn write_service_node_table<W: Write>(
    writer: &mut W,
    nodes: &[StoredNode],
    colors: &CliColors,
) -> grm_rs::Result<()> {
    let property_names = nodes
        .iter()
        .flat_map(|node| node.props.keys().cloned())
        .collect::<BTreeSet<_>>();
    let mut headers = vec!["id".to_string(), "labels".to_string()];
    headers.extend(property_names.iter().cloned());
    let header_refs = headers.iter().map(String::as_str).collect::<Vec<_>>();
    let header_kinds =
        std::iter::repeat_n(TableHeaderKind::Property, headers.len()).collect::<Vec<_>>();
    let rows = nodes
        .iter()
        .map(|node| {
            let mut row = vec![node.id.to_string(), node.labels.join(",")];
            row.extend(
                property_names
                    .iter()
                    .map(|name| service_table_value(node.props.get(name), colors)),
            );
            row
        })
        .collect::<Vec<_>>();
    write_cli_table(writer, &header_refs, &header_kinds, &rows, colors)
}

fn write_service_edge_table<W: Write>(
    writer: &mut W,
    edges: &[StoredRel],
    colors: &CliColors,
) -> grm_rs::Result<()> {
    let property_names = edges
        .iter()
        .flat_map(|edge| edge.props.keys().cloned())
        .collect::<BTreeSet<_>>();
    let mut headers = vec![
        "id".to_string(),
        "from".to_string(),
        "to".to_string(),
        "type".to_string(),
    ];
    headers.extend(property_names.iter().cloned());
    let header_refs = headers.iter().map(String::as_str).collect::<Vec<_>>();
    let header_kinds =
        std::iter::repeat_n(TableHeaderKind::Property, headers.len()).collect::<Vec<_>>();
    let rows = edges
        .iter()
        .map(|edge| {
            let mut row = vec![
                edge.id.to_string(),
                edge.from.to_string(),
                edge.to.to_string(),
                colors.type_name(&edge.rel_type),
            ];
            row.extend(
                property_names
                    .iter()
                    .map(|name| service_table_value(edge.props.get(name), colors)),
            );
            row
        })
        .collect::<Vec<_>>();
    write_cli_table(writer, &header_refs, &header_kinds, &rows, colors)
}

fn write_service_explain<W: Write>(
    writer: &mut W,
    explain: &proto::ExplainResponse,
) -> grm_rs::Result<()> {
    writeln!(writer, "Current logical plan for {}", explain.plan_kind)?;
    write_service_plan_steps(writer, &explain.steps)?;
    Ok(())
}

fn write_service_profile<W: Write>(
    writer: &mut W,
    profile: &proto::ProfileResponse,
) -> grm_rs::Result<()> {
    let plan = profile.plan.as_ref().ok_or_else(|| {
        grm_rs::GrmError::Constraint("service profile response is missing plan".into())
    })?;
    writeln!(writer, "Profile for {}", plan.plan_kind)?;
    write_service_plan_steps(writer, &plan.steps)?;
    writeln!(writer, "Result rows: {}", profile.row_count)?;
    writeln!(writer, "Elapsed: {}us", profile.elapsed_micros)?;
    Ok(())
}

fn write_service_plan_steps<W: Write>(writer: &mut W, steps: &[String]) -> grm_rs::Result<()> {
    writeln!(writer, "Plan steps:")?;
    for (index, step) in steps.iter().enumerate() {
        writeln!(writer, "  {}. {}", index + 1, step)?;
    }
    Ok(())
}

fn props_display(props: &BTreeMap<String, Value>, colors: &CliColors) -> String {
    if props.is_empty() {
        return "{}".to_string();
    }
    let values = props
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                colors.property_name(key),
                scalar_display(value, colors)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{{{values}}}")
}

fn scalar_display(value: &Value, colors: &CliColors) -> String {
    match value {
        Value::String(value) if value.contains(char::is_whitespace) => {
            colors.string_value(&format!("\"{value}\""))
        }
        Value::String(value) => colors.string_value(value),
        _ => value.to_string(),
    }
}

fn service_table_value(value: Option<&Value>, colors: &CliColors) -> String {
    value
        .map(|value| scalar_display(value, colors))
        .unwrap_or_default()
}

fn write_cli_table<W: Write>(
    writer: &mut W,
    headers: &[&str],
    header_kinds: &[TableHeaderKind],
    rows: &[Vec<String>],
    colors: &CliColors,
) -> grm_rs::Result<()> {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(visible_width(cell));
        }
    }

    let border = format_table_border(&widths);
    writeln!(writer, "{border}")?;
    writeln!(
        writer,
        "{}",
        format_table_header_row(headers, header_kinds, &widths, colors)
    )?;
    writeln!(writer, "{border}")?;
    for row in rows {
        writeln!(writer, "{}", format_table_row(row, &widths))?;
    }
    writeln!(writer, "{border}")?;
    Ok(())
}

fn format_table_border(widths: &[usize]) -> String {
    let mut line = String::new();
    line.push('+');
    for width in widths {
        line.push_str(&"-".repeat(*width + 2));
        line.push('+');
    }
    line
}

fn format_table_header_row(
    headers: &[&str],
    header_kinds: &[TableHeaderKind],
    widths: &[usize],
    colors: &CliColors,
) -> String {
    let styled = headers
        .iter()
        .zip(header_kinds.iter())
        .map(|(header, kind)| match kind {
            TableHeaderKind::Plain => (*header).to_string(),
            TableHeaderKind::Property => colors.property_name(header),
            TableHeaderKind::Type => colors.type_name(header),
        })
        .collect::<Vec<_>>();
    format_table_row(&styled, widths)
}

fn format_table_row(cells: &[String], widths: &[usize]) -> String {
    let mut line = String::new();
    line.push('|');
    for (cell, width) in cells.iter().zip(widths.iter()) {
        let padding = width.saturating_sub(visible_width(cell));
        line.push(' ');
        line.push_str(cell);
        line.push_str(&" ".repeat(padding));
        line.push_str(" |");
    }
    line
}

#[derive(Debug, Clone, Copy)]
enum TableHeaderKind {
    Plain,
    Property,
    Type,
}

#[derive(Debug, Clone, Copy)]
struct CliColors {
    enabled: bool,
}

impl CliColors {
    const GREEN: &'static str = "\x1b[32m";
    const BLUE: &'static str = "\x1b[34m";
    const ORANGE: &'static str = "\x1b[38;5;208m";
    const RESET: &'static str = "\x1b[0m";

    fn for_terminal(enabled: bool) -> Self {
        Self { enabled }
    }

    fn type_name(&self, text: &str) -> String {
        self.wrap(text, Self::GREEN)
    }

    fn property_name(&self, text: &str) -> String {
        self.wrap(text, Self::BLUE)
    }

    fn string_value(&self, text: &str) -> String {
        self.wrap(text, Self::ORANGE)
    }

    fn wrap(&self, text: &str, color: &str) -> String {
        if self.enabled {
            format!("{color}{text}{}", Self::RESET)
        } else {
            text.to_string()
        }
    }
}

fn visible_width(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut width = 0;

    while index < bytes.len() {
        if bytes[index] == 0x1b {
            index += 1;
            if index < bytes.len() && bytes[index] == b'[' {
                index += 1;
                while index < bytes.len() && bytes[index] != b'm' {
                    index += 1;
                }
                if index < bytes.len() {
                    index += 1;
                }
                continue;
            }
        }

        if let Some(ch) = text[index..].chars().next() {
            width += 1;
            index += ch.len_utf8();
        } else {
            break;
        }
    }

    width
}

fn parse_model_define_args(args: Vec<String>) -> grm_rs::Result<DefineNodeRequest> {
    if args.len() < 2 {
        return Err(grm_rs::GrmError::Constraint(
            "usage: model.define <Name> <id_field> [field:type:required|optional ...]".into(),
        ));
    }
    Ok(DefineNodeRequest {
        name: args[0].clone(),
        id_field: args[1].clone(),
        fields: parse_field_args(&args[2..])?,
    })
}

fn parse_link_define_args(args: Vec<String>) -> grm_rs::Result<DefineEdgeRequest> {
    if args.len() < 4 {
        return Err(grm_rs::GrmError::Constraint(
            "usage: link.define <Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]".into(),
        ));
    }
    Ok(DefineEdgeRequest {
        name: args[0].clone(),
        from_model: args[1].clone(),
        to_model: args[2].clone(),
        id_field: args[3].clone(),
        fields: parse_field_args(&args[4..])?,
    })
}

fn parse_field_args(args: &[String]) -> grm_rs::Result<Vec<FieldSpec>> {
    args.iter()
        .map(|arg| {
            let parts = arg.split(':').collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err(grm_rs::GrmError::Constraint(format!(
                    "field spec '{arg}' must be name:type:required|optional"
                )));
            }
            Ok(FieldSpec {
                name: parts[0].to_string(),
                value_type: match parts[1] {
                    "string" => FieldValueType::String,
                    "int" => FieldValueType::Int,
                    "float" => FieldValueType::Float,
                    "bool" => FieldValueType::Bool,
                    other => {
                        return Err(grm_rs::GrmError::Constraint(format!(
                            "unsupported field type '{other}'"
                        )));
                    }
                },
                required: match parts[2] {
                    "required" => true,
                    "optional" => false,
                    other => {
                        return Err(grm_rs::GrmError::Constraint(format!(
                            "unsupported field requirement '{other}'"
                        )));
                    }
                },
            })
        })
        .collect()
}

fn assignments_to_json(
    assignments: Vec<KeyValueArg>,
    _bindings: &BTreeMap<String, i64>,
) -> grm_rs::Result<BTreeMap<String, Value>> {
    assignments
        .into_iter()
        .map(|arg| Ok((arg.key, parse_scalar(&arg.value))))
        .collect()
}

fn terms_to_json_filters(
    terms: Vec<grm_rs::QueryTerm>,
    bindings: &BTreeMap<String, i64>,
    resolve_endpoint_ids: bool,
) -> grm_rs::Result<BTreeMap<String, Value>> {
    terms
        .into_iter()
        .map(|term| {
            let value = if resolve_endpoint_ids && matches!(term.key.as_str(), "from" | "to") {
                parse_scalar_or_binding(&term.value, bindings)
            } else {
                parse_scalar(&term.value)
            };
            Ok((term.key, value))
        })
        .collect()
}

fn parse_scalar_or_binding(raw: &str, bindings: &BTreeMap<String, i64>) -> Value {
    bindings
        .get(raw)
        .copied()
        .map(Value::from)
        .unwrap_or_else(|| parse_scalar(raw))
}

fn parse_scalar(raw: &str) -> Value {
    if raw == "true" {
        Value::Bool(true)
    } else if raw == "false" {
        Value::Bool(false)
    } else if let Ok(value) = raw.parse::<i64>() {
        Value::from(value)
    } else if let Ok(value) = raw.parse::<f64>() {
        Value::from(value)
    } else {
        Value::String(raw.to_string())
    }
}

fn take_required_id_or_binding(
    props: &mut BTreeMap<String, Value>,
    key: &str,
    bindings: &BTreeMap<String, i64>,
) -> grm_rs::Result<i64> {
    let value = props
        .remove(key)
        .ok_or_else(|| grm_rs::GrmError::Constraint(format!("edge.create requires {key}=<id>")))?;
    match value {
        Value::String(binding) => bindings
            .get(&binding)
            .copied()
            .ok_or_else(|| grm_rs::GrmError::Constraint(format!("{key} must be an integer id"))),
        value => value
            .as_i64()
            .ok_or_else(|| grm_rs::GrmError::Constraint(format!("{key} must be an integer id"))),
    }
}

fn parse_i64(raw: &str, name: &str) -> grm_rs::Result<i64> {
    raw.parse::<i64>()
        .map_err(|_| grm_rs::GrmError::Constraint(format!("{name} must be an integer")))
}

fn parse_i64_or_binding(
    raw: &str,
    name: &str,
    bindings: &BTreeMap<String, i64>,
) -> grm_rs::Result<i64> {
    bindings
        .get(raw)
        .copied()
        .map(Ok)
        .unwrap_or_else(|| parse_i64(raw, name))
}

fn ensure_binding_available(bindings: &BTreeMap<String, i64>, binding: &str) -> grm_rs::Result<()> {
    if bindings.contains_key(binding) {
        return Err(grm_rs::GrmError::Constraint(format!(
            "binding '{binding}' already exists"
        )));
    }
    Ok(())
}

fn required_env(name: &str) -> grm_rs::Result<String> {
    std::env::var(name)
        .map_err(|_| grm_rs::GrmError::Constraint(format!("{name} must be set in gRPC mode")))
}

fn service_error(error: grm_service_api::GrpcWorkspaceClientError) -> grm_rs::GrmError {
    grm_rs::GrmError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    #[test]
    fn parses_fresh_session() {
        assert_eq!(
            parse_session_startup(vec![]).unwrap(),
            SessionStartup::Fresh
        );
    }

    #[test]
    fn parses_load_json_with_autocommit_on() {
        assert_eq!(
            parse_session_startup(vec![
                "--load".to_string(),
                "json".to_string(),
                "session.json".to_string(),
                "--autocommit".to_string(),
                "on".to_string(),
            ])
            .unwrap(),
            SessionStartup::Load {
                format: StartupLoadFormat::Json,
                path: PathBuf::from("session.json"),
                autocommit: StartupAutocommit::On,
            }
        );
    }

    #[test]
    fn rejects_autocommit_without_load() {
        assert!(
            parse_session_startup(vec!["--autocommit".to_string(), "on".to_string(),])
                .unwrap_err()
                .contains("--load")
        );
    }

    #[tokio::test]
    async fn service_command_adapter_routes_supported_commands_through_grpc() {
        let tempdir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service =
            grm_service_api::GrpcWorkspaceService::with_local_workspace_root(tempdir.path())
                .into_server();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        let mut client = GrpcWorkspaceClient::connect(
            format!("http://{addr}"),
            "cli-service-smoke",
            GrpcWorkspaceMode::Create,
        )
        .await
        .unwrap();
        let mut session = ServiceCliSession::new(
            &mut client,
            ServiceSessionInfo {
                endpoint: format!("http://{addr}"),
                mode: GrpcWorkspaceMode::Create,
                format: DurabilityFormat::Binary,
                format_explicit: false,
            },
            false,
        );
        let mut output = Vec::new();
        session
            .handle_command(
                &mut output,
                "model.define User userId name:string:required",
                false,
            )
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "model.define Post postId title:string:required",
                false,
            )
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "link.define AUTHORED User Post authoredId year:int:required",
                false,
            )
            .await
            .unwrap();
        session
            .handle_command(&mut output, "let alice = node.create User name=Ada", false)
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "let post = node.create Post title=Notes",
                false,
            )
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "edge.create AUTHORED from=alice to=post year=2026",
                false,
            )
            .await
            .unwrap();
        session
            .handle_command(&mut output, "node.find User name=Ada", false)
            .await
            .unwrap();
        session
            .handle_command(&mut output, "node.find User name=Ada format=jsonl", false)
            .await
            .unwrap();
        session
            .handle_command(&mut output, "node.find User name=Ada format=table", false)
            .await
            .unwrap();
        session
            .handle_command(&mut output, "edge.find AUTHORED from=alice", false)
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "edge.find AUTHORED from=alice format=jsonl",
                false,
            )
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "edge.find AUTHORED from=alice format=table",
                false,
            )
            .await
            .unwrap();
        let graph_error = session
            .handle_command(&mut output, "node.find User name=Ada format=graph", false)
            .await
            .unwrap_err();
        assert!(
            graph_error
                .to_string()
                .contains("does not currently include traversal path rows")
        );
        session
            .handle_command(&mut output, "node.create Post title=alice", false)
            .await
            .unwrap();
        session
            .handle_command(&mut output, "node.find Post title=alice", false)
            .await
            .unwrap();
        session
            .handle_command(&mut output, "session.describe", false)
            .await
            .unwrap();
        session
            .handle_command(&mut output, "session.help", false)
            .await
            .unwrap();

        drop(session);
        drop(client);
        shutdown_tx.send(()).unwrap();
        server.await.unwrap().unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Defined node model"));
        assert!(output.contains("Node User id=1 {name=Ada}"));
        assert!(output.contains(
            r#"{"id":1,"kind":"node","labels":["User"],"model":"User","props":{"name":"Ada"}}"#
        ));
        assert!(output.contains("| id | labels | name |"));
        assert!(output.contains("Node Post id=3 {title=alice}"));
        assert!(output.contains("Edge AUTHORED id=1 from=1 to=2 {year=2026}"));
        assert!(output.contains(
            r#"{"from":1,"id":1,"kind":"edge","model":"AUTHORED","props":{"year":2026},"to":2,"type":"AUTHORED"}"#
        ));
        assert!(output.contains("| id | from | to | type     | year |"));
        assert!(output.contains("Session Summary"));
        assert!(output.contains("Stored rows: 3 nodes, 1 edges"));
        assert!(output.contains("+------+----------+-------+"));
        assert!(output.contains("| node | User     | 1     |"));
        assert!(output.contains("| node | Post     | 2     |"));
        assert!(output.contains("| edge | AUTHORED | 1     |"));
        assert!(output.contains("Backend: gRPC workspace storage"));
        assert!(output.contains("Mode: create"));
        assert!(output.contains("Persistence format: binary (default)"));
        assert!(output.contains("Available commands in gRPC service mode:"));
        assert!(output.contains("let <name> = node.create"));
        assert!(output.contains("edge.create <LinkName> from=<id|binding> to=<id|binding>"));
        assert!(tempdir.path().join("cli-service-smoke.bin").exists());
    }

    #[test]
    fn visible_width_ignores_ansi_colors() {
        let colors = CliColors::for_terminal(true);
        assert_eq!(visible_width(&colors.type_name("User")), 4);
        assert_eq!(visible_width(&colors.property_name("name")), 4);
    }
}
