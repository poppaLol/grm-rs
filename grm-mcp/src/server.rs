use std::path::PathBuf;
use std::sync::Arc;

use grm_rs::{
    DurableOperation, GraphClient, GrmError, Neo4jBackend, Neo4jConfig, Result as GrmResult,
    SessionState,
};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::router::tool::ToolRouter;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::config::{AutocommitTarget, StartupOptions};
use crate::tools::to_mcp_error;

#[derive(Clone)]
pub struct GrmMcpServer {
    pub(crate) state: Arc<Mutex<SessionState>>,
    pub(crate) neo4j: Option<GraphClient<Neo4jBackend>>,
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
            autocommit: options.autocommit,
            export_json: options.export_json,
            tool_router: Self::tool_router(),
        })
    }

    pub async fn from_startup_options(options: StartupOptions) -> GrmResult<Self> {
        match std::env::var("GRM_BACKEND").ok().as_deref() {
            Some("neo4j") => Self::new_neo4j(options).await,
            Some("memory") | Some("inmemory") | Some("in-memory") | None => Self::new(options),
            Some(other) => Err(GrmError::Constraint(format!(
                "unsupported GRM_BACKEND '{other}'; expected 'neo4j' or omit it for in-memory"
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
                "startup load/import/export/autocommit options are not supported in Neo4j MCP mode yet; Neo4j durability comes from Neo4j and runtime schema is session-local"
                    .into(),
            ));
        }

        let config = Neo4jConfig {
            uri: required_env("NEO4J_URI")?,
            user: required_env("NEO4J_USER")?,
            password: required_env("NEO4J_PASSWORD")?,
        };
        let backend = Neo4jBackend::connect(config).await?;
        Ok(Self {
            state: Arc::new(Mutex::new(SessionState::new())),
            neo4j: Some(GraphClient::new(backend)),
            autocommit: None,
            export_json: None,
            tool_router: Self::tool_router(),
        })
    }

    pub(crate) fn is_neo4j(&self) -> bool {
        self.neo4j.is_some()
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
                    "{tool} is not supported in Neo4j MCP mode yet; supported tools are grm_schema_list, grm_schema_define_node, grm_schema_define_edge, grm_node_create, grm_edge_create, simple grm_node_find, and simple grm_edge_find"
                ),
                None,
            )
        })
    }

    pub async fn schema_json(&self) -> Value {
        self.state.lock().await.schema_value()
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

fn required_env(name: &str) -> GrmResult<String> {
    std::env::var(name)
        .map_err(|_| GrmError::Constraint(format!("{name} must be set when GRM_BACKEND=neo4j")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{GrmMcpServer, StartupOptions};

    #[tokio::test]
    async fn schema_resource_starts_empty() {
        let server = GrmMcpServer::new(StartupOptions::default()).unwrap();
        let schema = server.schema_json().await;
        assert_eq!(schema["nodes"], json!([]));
        assert_eq!(schema["edges"], json!([]));
    }
}
