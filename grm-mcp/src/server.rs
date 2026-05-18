use std::path::PathBuf;
use std::sync::Arc;

use grm_rs::{DurableOperation, Result as GrmResult, SessionState};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::router::tool::ToolRouter;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::config::{AutocommitTarget, StartupOptions};
use crate::tools::to_mcp_error;

#[derive(Clone)]
pub struct GrmMcpServer {
    pub(crate) state: Arc<Mutex<SessionState>>,
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
            autocommit: options.autocommit,
            export_json: options.export_json,
            tool_router: Self::tool_router(),
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
