use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;

use grm_rs::runtime::{KeyValueArg, SessionCommand, parse_command_line};
use grm_rs::{
    CliSession, DefineEdgeRequest, DefineNodeRequest, EdgeCreateRequest, EdgeDeleteRequest,
    EdgeFindRequest, EdgeUpdateRequest, FieldSpec, FieldValueType, NodeCreateRequest,
    NodeDeleteRequest, NodeFindRequest, NodeUpdateRequest, RuntimeNodeModel, RuntimeRelModel,
    StoredNode, StoredRel,
};
use grm_service_api::{DurabilityFormat, GrpcWorkspaceClient, GrpcWorkspaceMode};
use serde_json::Value;

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
                if !matches!(startup, SessionStartup::Fresh) {
                    eprintln!(
                        "--script and --load are local CLI startup options and are not supported in gRPC service mode"
                    );
                    std::process::exit(1);
                }
                let stdin = io::stdin();
                let reader = BufReader::new(stdin.lock());
                if let Err(err) = run_service_session(reader, writer).await {
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
    "Usage: grm session [--script <path> | --load json|bin <path> [--autocommit on|off]]\nSet GRM_BACKEND=grpc with GRM_SERVICE_ENDPOINT, GRM_WORKSPACE_REF, and optional GRM_SERVICE_WORKSPACE_MODE=create|open to route supported commands through the gRPC workspace service."
}

async fn run_service_session<R: BufRead, W: Write>(reader: R, mut writer: W) -> grm_rs::Result<()> {
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
    let format = match std::env::var("GRM_SERVICE_WORKSPACE_FORMAT")
        .ok()
        .as_deref()
    {
        Some("json") => DurabilityFormat::Json,
        Some("bin" | "binary") | None => DurabilityFormat::Binary,
        Some(other) => {
            return Err(grm_rs::GrmError::Constraint(format!(
                "unsupported GRM_SERVICE_WORKSPACE_FORMAT '{other}'; expected 'json', 'bin', or 'binary'"
            )));
        }
    };
    let mut client =
        GrpcWorkspaceClient::connect_with_format(endpoint, workspace_ref, mode, format)
            .await
            .map_err(service_error)?;

    writeln!(
        writer,
        "Welcome to GRM-RS CLI.\ngRPC workspace service session ready. Supported commands route through ExecuteWorkspace."
    )?;
    let mut session = ServiceCliSession::new(&mut client);
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
        match session.handle_command(&mut writer, trimmed).await {
            Ok(true) => break,
            Ok(false) => {}
            Err(err) => writeln!(writer, "{err}")?,
        }
    }
    Ok(())
}

struct ServiceCliSession<'a> {
    client: &'a mut GrpcWorkspaceClient,
    bindings: BTreeMap<String, i64>,
}

impl<'a> ServiceCliSession<'a> {
    fn new(client: &'a mut GrpcWorkspaceClient) -> ServiceCliSession<'a> {
        ServiceCliSession {
            client,
            bindings: BTreeMap::new(),
        }
    }

    async fn handle_command<W: Write>(
        &mut self,
        writer: &mut W,
        line: &str,
    ) -> grm_rs::Result<bool> {
        match parse_command_line(line)? {
            SessionCommand::Help => write_service_help(writer)?,
            SessionCommand::Exit => return Ok(true),
            SessionCommand::SessionDescribe { .. }
            | SessionCommand::ModelList
            | SessionCommand::LinkList => {
                let schema = self.client.schema_list().await.map_err(service_error)?;
                write_service_schema(writer, &schema.node_models, &schema.edge_models)?;
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
                write_node(writer, &node)?;
            }
            SessionCommand::NodeFind { model_name, terms } => {
                let request = NodeFindRequest::from_adapter_filter_values(
                    model_name,
                    terms_to_json_filters(terms, &self.bindings)?,
                )?;
                let found = self
                    .client
                    .find_nodes(request)
                    .await
                    .map_err(service_error)?;
                for node in found.nodes {
                    write_node(writer, &node)?;
                }
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
                write_node(writer, &node)?;
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
                let from = take_required_id(&mut props, "from")?;
                let to = take_required_id(&mut props, "to")?;
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
                write_edge(writer, &edge)?;
            }
            SessionCommand::EdgeFind { model_name, terms } => {
                let request = EdgeFindRequest::from_adapter_filter_values(
                    model_name,
                    terms_to_json_filters(terms, &self.bindings)?,
                )?;
                let found = self
                    .client
                    .find_edges(request)
                    .await
                    .map_err(service_error)?;
                for edge in found.edges {
                    write_edge(writer, &edge)?;
                }
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
                write_edge(writer, &edge)?;
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
            | SessionCommand::LinkShow { .. }
            | SessionCommand::SessionExplainNodeFind { .. }
            | SessionCommand::SessionProfileNodeFind { .. }
            | SessionCommand::SessionExplainEdgeFind { .. }
            | SessionCommand::SessionProfileEdgeFind { .. } => {
                writeln!(
                    writer,
                    "Command is local-only or not supported in gRPC service CLI mode yet"
                )?;
            }
        }
        Ok(false)
    }
}

fn write_service_help<W: Write>(writer: &mut W) -> grm_rs::Result<()> {
    writeln!(writer, "Supported gRPC service commands:")?;
    writeln!(
        writer,
        "  model.define <Name> <id_field> [field:type:required|optional ...]"
    )?;
    writeln!(
        writer,
        "  link.define <Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]"
    )?;
    writeln!(writer, "  model.list | link.list | session.describe")?;
    writeln!(
        writer,
        "  node.create/find/update/delete and edge.create/find/update/delete"
    )?;
    writeln!(writer, "  session.exit")?;
    Ok(())
}

fn write_service_schema<W: Write>(
    writer: &mut W,
    nodes: &[RuntimeNodeModel],
    edges: &[RuntimeRelModel],
) -> grm_rs::Result<()> {
    writeln!(writer, "Service Schema")?;
    for node in nodes {
        writeln!(
            writer,
            "| node | {} | id={} |",
            node.name, node.id_field_name
        )?;
    }
    for edge in edges {
        writeln!(
            writer,
            "| edge | {} | {} -> {} | id={} |",
            edge.name, edge.from_model, edge.to_model, edge.id_field_name
        )?;
    }
    Ok(())
}

fn write_node<W: Write>(writer: &mut W, node: &StoredNode) -> grm_rs::Result<()> {
    writeln!(
        writer,
        "Node {} id={} {}",
        node.labels.first().map(String::as_str).unwrap_or(""),
        node.id,
        props_display(&node.props)
    )?;
    Ok(())
}

fn write_edge<W: Write>(writer: &mut W, edge: &StoredRel) -> grm_rs::Result<()> {
    writeln!(
        writer,
        "Edge {} id={} from={} to={} {}",
        edge.rel_type,
        edge.id,
        edge.from,
        edge.to,
        props_display(&edge.props)
    )?;
    Ok(())
}

fn props_display(props: &BTreeMap<String, Value>) -> String {
    let values = props
        .iter()
        .map(|(key, value)| format!("{key}={}", scalar_display(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{values}}}")
}

fn scalar_display(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
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
    bindings: &BTreeMap<String, i64>,
) -> grm_rs::Result<BTreeMap<String, Value>> {
    assignments
        .into_iter()
        .map(|arg| Ok((arg.key, parse_scalar_or_binding(&arg.value, bindings))))
        .collect()
}

fn terms_to_json_filters(
    terms: Vec<grm_rs::QueryTerm>,
    bindings: &BTreeMap<String, i64>,
) -> grm_rs::Result<BTreeMap<String, Value>> {
    terms
        .into_iter()
        .map(|term| Ok((term.key, parse_scalar_or_binding(&term.value, bindings))))
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

fn take_required_id(props: &mut BTreeMap<String, Value>, key: &str) -> grm_rs::Result<i64> {
    let value = props
        .remove(key)
        .ok_or_else(|| grm_rs::GrmError::Constraint(format!("edge.create requires {key}=<id>")))?;
    value
        .as_i64()
        .ok_or_else(|| grm_rs::GrmError::Constraint(format!("{key} must be an integer id")))
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
        let mut session = ServiceCliSession::new(&mut client);
        let mut output = Vec::new();
        session
            .handle_command(&mut output, "model.define User userId name:string:required")
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "model.define Post postId title:string:required",
            )
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "link.define Authored User Post authoredId year:int:required",
            )
            .await
            .unwrap();
        session
            .handle_command(&mut output, "let alice = node.create User name=Ada")
            .await
            .unwrap();
        session
            .handle_command(&mut output, "let post = node.create Post title=Notes")
            .await
            .unwrap();
        session
            .handle_command(
                &mut output,
                "edge.create Authored from=alice to=post year=2026",
            )
            .await
            .unwrap();
        session
            .handle_command(&mut output, "node.find User name=Ada")
            .await
            .unwrap();
        session
            .handle_command(&mut output, "edge.find Authored from=alice")
            .await
            .unwrap();

        drop(session);
        drop(client);
        shutdown_tx.send(()).unwrap();
        server.await.unwrap().unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Defined node model"));
        assert!(output.contains("Node User id=1 {name=Ada}"));
        assert!(output.contains("Edge Authored id=1 from=1 to=2 {year=2026}"));
        assert!(tempdir.path().join("cli-service-smoke.bin").exists());
    }
}
