use std::path::PathBuf;
use std::sync::Arc;

use grm_rs::{
    DurableOperation, GraphClient, GrmError, Neo4jBackend, Neo4jConfig, Result as GrmResult,
    SessionState,
};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::router::tool::ToolRouter;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::config::{AutocommitTarget, StartupOptions};
use crate::service::{ServiceMcpBackend, ServiceWorkspaceFormat, ServiceWorkspaceMode};
use crate::tools::to_mcp_error;

#[derive(Clone)]
pub struct GrmMcpServer {
    pub(crate) state: Arc<Mutex<SessionState>>,
    pub(crate) neo4j: Option<GraphClient<Neo4jBackend>>,
    pub(crate) service: Option<ServiceMcpBackend>,
    pub(crate) schema_template_source: Option<PathBuf>,
    pub(crate) schema_template_loaded_from_file: bool,
    pub(crate) autocommit: Option<AutocommitTarget>,
    pub(crate) export_json: Option<PathBuf>,
    #[allow(dead_code)]
    pub(crate) tool_router: ToolRouter<Self>,
}

impl GrmMcpServer {
    pub fn new(options: StartupOptions) -> GrmResult<Self> {
        let mut state = SessionState::new();
        let has_startup_source = options.load_json.is_some()
            || options.load_bin.is_some()
            || options.import_json.is_some();
        if let Some(path) = &options.load_json {
            state.load_from_json(path)?;
        }
        if let Some(path) = &options.load_bin {
            state.load_from_binary(path)?;
        }
        if let Some(path) = &options.import_json {
            state.import_from_json(path)?;
        }

        if let Some(target) = &options.autocommit {
            if !has_startup_source && target.path.exists() {
                state.recover_durable(target.format.durability_format(), &target.path)?;
            } else {
                state.checkpoint_durable(target.format.durability_format(), &target.path)?;
            }
        }

        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            neo4j: None,
            service: None,
            schema_template_source: None,
            schema_template_loaded_from_file: false,
            autocommit: options.autocommit,
            export_json: options.export_json,
            tool_router: Self::tool_router(),
        })
    }

    pub async fn from_startup_options(options: StartupOptions) -> GrmResult<Self> {
        match std::env::var("GRM_BACKEND").ok().as_deref() {
            Some("neo4j") => Self::new_neo4j(options).await,
            Some("grpc") => Self::new_service(options).await,
            Some("memory") | Some("inmemory") | Some("in-memory") | None => Self::new(options),
            Some(other) => Err(GrmError::Constraint(format!(
                "unsupported GRM_BACKEND '{other}'; expected 'neo4j', 'grpc', or omit it for in-memory"
            ))),
        }
    }

    async fn new_neo4j(options: StartupOptions) -> GrmResult<Self> {
        if options.load_json.is_some()
            || options.load_bin.is_some()
            || options.import_json.is_some()
            || options.autocommit.is_some()
            || options.export_json.is_some()
        {
            return Err(GrmError::NotSupported(
                "startup load/import/export/autocommit options are not supported in Neo4j MCP mode yet; Neo4j durability comes from Neo4j and runtime schema is session-local",
            ));
        }

        let config = Neo4jConfig {
            uri: required_env("NEO4J_URI")?,
            user: required_env("NEO4J_USER")?,
            password: required_env("NEO4J_PASSWORD")?,
        };
        let schema_template_source = optional_path_env("GRM_SCHEMA_TEMPLATE")?;
        let (state, schema_template_loaded_from_file) =
            initialize_schema_memory(schema_template_source.as_deref())?;
        let backend = Neo4jBackend::connect(config).await?;
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            neo4j: Some(GraphClient::new(backend)),
            service: None,
            schema_template_source,
            schema_template_loaded_from_file,
            autocommit: None,
            export_json: None,
            tool_router: Self::tool_router(),
        })
    }

    async fn new_service(options: StartupOptions) -> GrmResult<Self> {
        if options.load_json.is_some()
            || options.load_bin.is_some()
            || options.import_json.is_some()
            || options.autocommit.is_some()
            || options.export_json.is_some()
        {
            return Err(GrmError::NotSupported(
                "startup load/import/export/autocommit options are not supported in gRPC MCP mode; workspace persistence is owned by the service",
            ));
        }

        let endpoint = required_env("GRM_SERVICE_ENDPOINT")?;
        let workspace_ref = required_env("GRM_WORKSPACE_REF")?;
        let mode = match std::env::var("GRM_SERVICE_WORKSPACE_MODE") {
            Ok(value) => ServiceWorkspaceMode::parse(value.trim())?,
            Err(std::env::VarError::NotPresent) => ServiceWorkspaceMode::Open,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(GrmError::Constraint(
                    "GRM_SERVICE_WORKSPACE_MODE must be valid Unicode".into(),
                ));
            }
        };
        let format = match std::env::var("GRM_SERVICE_WORKSPACE_FORMAT") {
            Ok(value) => ServiceWorkspaceFormat::parse(value.trim())?,
            Err(std::env::VarError::NotPresent) => ServiceWorkspaceFormat::Binary,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(GrmError::Constraint(
                    "GRM_SERVICE_WORKSPACE_FORMAT must be valid Unicode".into(),
                ));
            }
        };
        let service = ServiceMcpBackend::connect(endpoint, workspace_ref, mode, format).await?;
        Ok(Self {
            state: Arc::new(Mutex::new(SessionState::new())),
            neo4j: None,
            service: Some(service),
            schema_template_source: None,
            schema_template_loaded_from_file: false,
            autocommit: None,
            export_json: None,
            tool_router: Self::tool_router(),
        })
    }

    pub(crate) fn is_neo4j(&self) -> bool {
        self.neo4j.is_some()
    }

    pub(crate) fn is_service(&self) -> bool {
        self.service.is_some()
    }

    pub(crate) fn service_backend(&self) -> GrmResult<&ServiceMcpBackend> {
        self.service
            .as_ref()
            .ok_or_else(|| GrmError::Backend("server is not running in gRPC service mode".into()))
    }

    pub(crate) fn neo4j_client(&self) -> GrmResult<&GraphClient<Neo4jBackend>> {
        self.neo4j
            .as_ref()
            .ok_or_else(|| GrmError::Backend("server is not running in Neo4j mode".into()))
    }

    pub(crate) fn unsupported_in_neo4j(&self, tool: &str) -> Option<McpError> {
        self.is_neo4j().then(|| {
            McpError::internal_error(
                format!(
                    "{tool} is not supported in Neo4j MCP mode yet; supported tools are grm_schema_list, grm_schema_define_node, grm_schema_define_edge, grm_batch for schema/node/edge create/update/delete, grm_node_create, grm_node_update, grm_node_delete, grm_edge_create, grm_edge_update, grm_edge_delete, simple grm_node_find, and simple grm_edge_find"
                ),
                None,
            )
        })
    }

    pub(crate) fn unsupported_in_service(&self, tool: &str) -> Option<McpError> {
        self.is_service().then(|| {
            McpError::internal_error(
                format!(
                    "{tool} is not supported in gRPC MCP mode yet; supported tools are grm_schema_list, grm_schema_define_node, grm_schema_define_edge, grm_batch for schema/node/edge create/update/delete, grm_node_create, grm_node_update, grm_node_delete, grm_edge_create, grm_edge_update, grm_edge_delete, simple grm_node_find, and simple grm_edge_find"
                ),
                None,
            )
        })
    }

    pub async fn schema_json(&self) -> GrmResult<Value> {
        if let Some(service) = &self.service {
            return service.schema_json().await;
        }
        let state = self.state.lock().await;
        let mut value = state.schema_value();
        if self.is_neo4j() {
            let node_count = state.catalog().list_node_models().len();
            let edge_count = state.catalog().list_rel_models().len();
            value["backend"] = json!({
                "mode": "neo4j",
                "connected": true,
                "runtime_schema_model_count": node_count + edge_count,
                "runtime_schema_empty": node_count == 0 && edge_count == 0,
                "schema_template_loaded": self.schema_template_loaded_from_file,
                "schema_template_persistence_enabled": self.schema_template_source.is_some(),
                "schema_template_source": self.schema_template_source.as_ref().map(|path| path.display().to_string()),
                "schema_memory_loaded_from_file": self.schema_template_loaded_from_file,
                "schema_memory_persistence_enabled": self.schema_template_source.is_some(),
                "schema_memory_source": self.schema_template_source.as_ref().map(|path| path.display().to_string()),
                "note": "Neo4j graph data may already exist outside this session-local runtime schema."
            });
            if node_count == 0 && edge_count == 0 {
                value["guidance"] = json!({
                "schema_required": "Define or reconstruct session-local runtime schema before creating or finding typed Neo4j data.",
                    "startup_flow": [
                        "Call grm_schema_list.",
                        "Read grm://backend/status for backend/session orientation.",
                        "If schema is empty, ask whether to define a fresh schema or reconstruct one from project docs.",
                        "Only then perform grm_batch writes."
                    ]
                });
            }
        }
        Ok(value)
    }

    pub async fn backend_status_json(&self) -> Value {
        if let Some(service) = &self.service {
            return service.status_value();
        }
        let state = self.state.lock().await;
        let node_count = state.catalog().list_node_models().len();
        let edge_count = state.catalog().list_rel_models().len();
        if self.is_neo4j() {
            neo4j_backend_status_value(
                node_count,
                edge_count,
                self.schema_template_source.as_ref(),
                self.schema_template_loaded_from_file,
            )
        } else {
            json!({
                "backend": {
                    "mode": "in-memory",
                    "connected": true,
                    "runtime_schema_model_count": node_count + edge_count,
                    "runtime_schema_empty": node_count == 0 && edge_count == 0
                }
            })
        }
    }

    pub async fn export_json(&self) -> GrmResult<Value> {
        self.state.lock().await.export_value()
    }

    pub async fn summary_json(&self) -> Value {
        self.state.lock().await.summary_value()
    }

    pub(crate) async fn persist_autocommit(&self, state: &SessionState) -> GrmResult<()> {
        let Some(target) = &self.autocommit else {
            return self.persist_export(state).await;
        };

        state.checkpoint_durable(target.format.durability_format(), &target.path)?;

        self.persist_export(state).await
    }

    pub(crate) async fn append_autocommit_ops(
        &self,
        state: &SessionState,
        ops: &[DurableOperation],
    ) -> GrmResult<()> {
        let Some(target) = &self.autocommit else {
            return self.persist_export(state).await;
        };

        for op in ops {
            state.append_durable_operation(&target.path, op)?;
        }

        self.persist_export(state).await
    }

    pub(crate) fn append_schema_template_ops(
        &self,
        state: &SessionState,
        ops: &[DurableOperation],
    ) -> GrmResult<()> {
        let Some(path) = &self.schema_template_source else {
            return Ok(());
        };

        for op in ops {
            state.append_durable_operation(path, op)?;
        }
        Ok(())
    }

    pub(crate) async fn persist_export(&self, state: &SessionState) -> GrmResult<()> {
        let Some(path) = &self.export_json else {
            return Ok(());
        };

        state.export_to_json(path)
    }

    pub(crate) async fn with_state_mut<T>(
        &self,
        persist: bool,
        work: impl AsyncFnOnce(&mut SessionState) -> GrmResult<T>,
    ) -> Result<T, McpError> {
        let mut state = self.state.lock().await;
        let value = work(&mut state).await.map_err(to_mcp_error)?;
        if persist {
            self.persist_autocommit(&state)
                .await
                .map_err(to_mcp_error)?;
        }
        Ok(value)
    }

    pub(crate) async fn with_state_mut_durable<T>(
        &self,
        work: impl AsyncFnOnce(&mut SessionState) -> GrmResult<(T, Vec<DurableOperation>)>,
    ) -> Result<T, McpError> {
        let mut state = self.state.lock().await;
        let (value, ops) = work(&mut state).await.map_err(to_mcp_error)?;
        self.append_autocommit_ops(&state, &ops)
            .await
            .map_err(to_mcp_error)?;
        Ok(value)
    }
}

fn neo4j_backend_status_value(
    node_count: usize,
    edge_count: usize,
    schema_template_source: Option<&PathBuf>,
    schema_template_loaded_from_file: bool,
) -> Value {
    json!({
        "backend": {
            "mode": "neo4j",
            "connected": true,
            "runtime_schema_model_count": node_count + edge_count,
            "runtime_schema_empty": node_count == 0 && edge_count == 0,
            "schema_template_loaded": schema_template_loaded_from_file,
            "schema_template_persistence_enabled": schema_template_source.is_some(),
            "schema_template_source": schema_template_source.map(|path| path.display().to_string()),
            "schema_memory_loaded_from_file": schema_template_loaded_from_file,
            "schema_memory_persistence_enabled": schema_template_source.is_some(),
            "schema_memory_source": schema_template_source.map(|path| path.display().to_string()),
            "note": "Runtime schema metadata is session-local; GRM_SCHEMA_TEMPLATE can back it with a local GRM session file while Neo4j stores graph data.",
            "supported_tools": [
                "grm_schema_list",
                "grm_schema_define_node",
                "grm_schema_define_edge",
                "grm_batch",
                "grm_node_create",
                "grm_node_update",
                "grm_node_delete",
                "grm_edge_create",
                "grm_edge_update",
                "grm_edge_delete",
                "grm_node_find",
                "grm_edge_find"
            ],
            "unsupported_surfaces": [
                "snapshots",
                "import/export",
                "autocommit",
                "explain/profile",
                "traversal/query parity"
            ],
            "future_orientation_tools": ["grm_backend_status", "grm_store_summary", "grm_schema_introspect"]
        },
        "recommended_startup_flow": [
            "Call grm_schema_list.",
            "Inspect this backend/session status resource.",
            "If schema_template_loaded is true, verify grm_schema_list contains the intended recovered models and fields before writing.",
            "If schema_template_persistence_enabled is true and schema_template_loaded is false, this server started fresh and will persist schema definitions to the configured local file.",
            "If schema is empty, ask the user whether to define a fresh schema or reconstruct one from project docs.",
            "Only then perform grm_batch writes."
        ]
    })
}

fn initialize_schema_memory(path: Option<&std::path::Path>) -> GrmResult<(SessionState, bool)> {
    let mut state = SessionState::new();
    let Some(path) = path else {
        return Ok((state, false));
    };

    if path.exists() {
        state.recover_durable(grm_rs::DurabilityFormat::Json, path)?;
        Ok((state, true))
    } else {
        state.checkpoint_durable(grm_rs::DurabilityFormat::Json, path)?;
        Ok((state, false))
    }
}

fn required_env(name: &str) -> GrmResult<String> {
    std::env::var(name)
        .map_err(|_| GrmError::Constraint(format!("{name} must be set for this MCP backend mode")))
}

fn optional_path_env(name: &str) -> GrmResult<Option<PathBuf>> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Err(GrmError::Constraint(format!(
            "{name} must not be empty when set"
        ))),
        Ok(value) => Ok(Some(PathBuf::from(value))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(GrmError::Constraint(format!(
            "{name} must be valid Unicode"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use grm_rs::{BackendIdType, DurableOperation, RuntimeNodeModel};
    use serde_json::json;

    use crate::{GrmMcpServer, StartupOptions};

    #[tokio::test]
    async fn schema_resource_starts_empty() {
        let server = GrmMcpServer::new(StartupOptions::default()).unwrap();
        let schema = server.schema_json().await.unwrap();
        assert_eq!(schema["nodes"], json!([]));
        assert_eq!(schema["edges"], json!([]));
    }

    #[test]
    fn neo4j_backend_status_reports_schema_template_metadata() {
        let source = std::path::PathBuf::from("project-memory-schema.json");
        let status = super::neo4j_backend_status_value(2, 1, Some(&source), true);

        assert_eq!(status["backend"]["mode"], json!("neo4j"));
        assert_eq!(status["backend"]["runtime_schema_model_count"], json!(3));
        assert_eq!(status["backend"]["runtime_schema_empty"], json!(false));
        assert_eq!(status["backend"]["schema_template_loaded"], json!(true));
        assert_eq!(
            status["backend"]["schema_template_persistence_enabled"],
            json!(true)
        );
        assert_eq!(
            status["backend"]["schema_memory_loaded_from_file"],
            json!(true)
        );
        assert_eq!(
            status["backend"]["schema_memory_persistence_enabled"],
            json!(true)
        );
        assert_eq!(
            status["backend"]["schema_template_source"],
            json!("project-memory-schema.json")
        );
        assert_eq!(
            status["backend"]["schema_memory_source"],
            json!("project-memory-schema.json")
        );
        assert!(
            status["backend"]["supported_tools"]
                .as_array()
                .unwrap()
                .contains(&json!("grm_node_update"))
        );
        assert!(
            status["backend"]["supported_tools"]
                .as_array()
                .unwrap()
                .contains(&json!("grm_edge_delete"))
        );
    }

    #[test]
    fn neo4j_backend_status_reports_absent_schema_template() {
        let status = super::neo4j_backend_status_value(0, 0, None, false);

        assert_eq!(status["backend"]["mode"], json!("neo4j"));
        assert_eq!(status["backend"]["runtime_schema_model_count"], json!(0));
        assert_eq!(status["backend"]["runtime_schema_empty"], json!(true));
        assert_eq!(status["backend"]["schema_template_loaded"], json!(false));
        assert_eq!(
            status["backend"]["schema_template_persistence_enabled"],
            json!(false)
        );
        assert_eq!(
            status["backend"]["schema_memory_persistence_enabled"],
            json!(false)
        );
        assert_eq!(status["backend"]["schema_template_source"], json!(null));
        assert_eq!(status["backend"]["schema_memory_source"], json!(null));
    }

    #[test]
    fn schema_memory_missing_file_starts_fresh_and_creates_checkpoint() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("schema-memory.json");

        let (state, loaded) = super::initialize_schema_memory(Some(&path)).unwrap();

        assert!(!loaded);
        assert!(path.exists());
        assert!(state.catalog().is_empty());
    }

    #[test]
    fn schema_memory_existing_file_recovers_schema_ops() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("schema-memory.json");
        let (state, loaded) = super::initialize_schema_memory(Some(&path)).unwrap();
        assert!(!loaded);

        let model =
            RuntimeNodeModel::new("RoadmapItem", "roadmapItemId", BackendIdType::Int64, vec![])
                .unwrap();
        state
            .append_durable_operation(
                &path,
                &DurableOperation::RegisterNodeModel {
                    model: model.clone(),
                },
            )
            .unwrap();

        let (recovered, loaded) = super::initialize_schema_memory(Some(&path)).unwrap();

        assert!(loaded);
        assert!(recovered.model("RoadmapItem").is_some());
    }
}
