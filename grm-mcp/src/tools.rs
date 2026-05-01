use std::io::Cursor;

use grm_rs::{CliSession, GrmError, RuntimeNodeModel, RuntimeRelModel};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    AnnotateAble, JsonObject, ListResourcesResult, PaginatedRequestParams, RawResource,
    ReadResourceRequestParams, ReadResourceResult, ResourceContents, ServerCapabilities,
    ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData as McpError, Json, RoleServer, ServerHandler, tool, tool_handler, tool_router,
};
use serde_json::json;

use crate::schema::{
    DefineEdgeParams, DefineNodeParams, EdgeCreateParams, EdgeDeleteParams, EdgeFindParams,
    EdgeUpdateParams, ExportParams, FileFormat, FileFormatParams, NodeCreateParams,
    NodeDeleteParams, NodeFindParams, NodeUpdateParams, PathParams, QueryParams, json_error,
    parse_fields, to_object, value_map_to_raw,
};
use crate::server::GrmMcpServer;

const QUERY_LANGUAGE_DOC: &str = include_str!("../../docs/query-language-design.md");

#[tool_router(vis = "pub(crate)")]
impl GrmMcpServer {
    #[tool(description = "Return the current GRM runtime schema and backend identity types.")]
    async fn grm_schema_list(&self) -> Result<Json<JsonObject>, McpError> {
        Ok(Json(to_object(self.schema_json().await)?))
    }

    #[tool(description = "Define a runtime node model in the current GRM session.")]
    async fn grm_schema_define_node(
        &self,
        Parameters(params): Parameters<DefineNodeParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            let model = RuntimeNodeModel::new(
                params.name,
                params.id_field,
                state.node_id_type(),
                parse_fields(params.fields)?,
            )?;
            state.register_model(model)?;
            Ok(state.schema_value())
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Define a runtime edge/link model in the current GRM session.")]
    async fn grm_schema_define_edge(
        &self,
        Parameters(params): Parameters<DefineEdgeParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            let model = RuntimeRelModel::new(
                params.name,
                params.from_model,
                params.to_model,
                params.id_field,
                state.rel_id_type(),
                parse_fields(params.fields)?,
            )?;
            state.register_rel_model(model)?;
            Ok(state.schema_value())
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Create a node instance for a runtime model.")]
    async fn grm_node_create(
        &self,
        Parameters(params): Parameters<NodeCreateParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            let props = value_map_to_raw(params.props)?;
            let node = state.create_instance(&params.model, &props).await?;
            serde_json::to_value(node).map_err(json_error)
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Update an existing node instance.")]
    async fn grm_node_update(
        &self,
        Parameters(params): Parameters<NodeUpdateParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            let props = value_map_to_raw(params.props)?;
            let node = state
                .update_node_instance(&params.model, &params.id.to_string(), &props)
                .await?;
            serde_json::to_value(node).map_err(json_error)
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Delete an existing node instance.")]
    async fn grm_node_delete(
        &self,
        Parameters(params): Parameters<NodeDeleteParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            state
                .delete_node_instance(&params.model, &params.id.to_string())
                .await?;
            Ok(json!({ "deleted": true, "model": params.model, "id": params.id }))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Find node instances using GRM query filter terms.")]
    async fn grm_node_find(
        &self,
        Parameters(params): Parameters<NodeFindParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(false, async |state| {
            let filters = value_map_to_raw(params.filters)?;
            let nodes = state.find_nodes(&params.model, &filters)?;
            Ok(json!({ "model": params.model, "nodes": nodes }))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Create an edge instance between two node ids.")]
    async fn grm_edge_create(
        &self,
        Parameters(params): Parameters<EdgeCreateParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            let props = value_map_to_raw(params.props)?;
            let edge = state
                .create_relationship_instance(
                    &params.model,
                    &params.from.to_string(),
                    &params.to.to_string(),
                    &props,
                )
                .await?;
            serde_json::to_value(edge).map_err(json_error)
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Update an existing edge instance.")]
    async fn grm_edge_update(
        &self,
        Parameters(params): Parameters<EdgeUpdateParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            let props = value_map_to_raw(params.props)?;
            let edge = state
                .update_relationship_instance(&params.model, &params.id.to_string(), &props)
                .await?;
            serde_json::to_value(edge).map_err(json_error)
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Delete an existing edge instance.")]
    async fn grm_edge_delete(
        &self,
        Parameters(params): Parameters<EdgeDeleteParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            state
                .delete_relationship_instance(&params.model, &params.id.to_string())
                .await?;
            Ok(json!({ "deleted": true, "model": params.model, "id": params.id }))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Find edge instances using GRM query filter terms.")]
    async fn grm_edge_find(
        &self,
        Parameters(params): Parameters<EdgeFindParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(false, async |state| {
            let filters = value_map_to_raw(params.filters)?;
            let edges = state.find_relationships(&params.model, &filters)?;
            Ok(json!({ "model": params.model, "edges": edges }))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(
        description = "Run one CLI-compatible GRM session command and return its rendered output."
    )]
    async fn grm_query(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        let mut state = self.state.lock().await;
        let current = std::mem::take(&mut *state);
        let mut session =
            CliSession::with_state(current, Cursor::new(Vec::<u8>::new()), Vec::new());
        let should_exit = session
            .handle_command(&params.command)
            .await
            .map_err(to_mcp_error)?;
        let (next_state, _, output) = session.into_parts();
        self.persist_autocommit(&next_state)
            .await
            .map_err(to_mcp_error)?;
        *state = next_state;
        Ok(Json(to_object(json!({
            "command": params.command,
            "should_exit": should_exit,
            "output": String::from_utf8_lossy(&output).to_string(),
        }))?))
    }

    #[tool(description = "Save the current GRM session to a JSON or binary session file.")]
    async fn grm_save(
        &self,
        Parameters(params): Parameters<FileFormatParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(false, async |state| {
            match params.format {
                FileFormat::Json => state.save_to_json(&params.path)?,
                FileFormat::Binary => state.save_to_binary(&params.path)?,
            }
            Ok(json!({ "saved": true, "format": params.format, "path": params.path }))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Load a GRM session from a JSON or binary session file.")]
    async fn grm_load(
        &self,
        Parameters(params): Parameters<FileFormatParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            match params.format {
                FileFormat::Json => state.load_from_json(&params.path)?,
                FileFormat::Binary => state.load_from_binary(&params.path)?,
            }
            Ok(state.summary_value())
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Import a GRM interchange JSON document into an empty session.")]
    async fn grm_import(
        &self,
        Parameters(params): Parameters<PathParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(true, async |state| {
            state.import_from_json(&params.path)?;
            Ok(state.summary_value())
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(
        description = "Export the current graph as GRM interchange JSON, optionally writing it to a path."
    )]
    async fn grm_export(
        &self,
        Parameters(params): Parameters<ExportParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        self.with_state_mut(false, async |state| {
            if let Some(path) = params.path {
                state.export_to_json(&path)?;
                Ok(json!({ "exported": true, "path": path, "document": state.export_value()? }))
            } else {
                state.export_value()
            }
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }
}

#[tool_handler]
impl ServerHandler for GrmMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions("Use GRM tools to inspect and mutate the local runtime graph session. Prefer structured tools over raw CLI commands when possible.")
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                RawResource::new("grm://schema", "schema").no_annotation(),
                RawResource::new("grm://graph/export", "graph export").no_annotation(),
                RawResource::new("grm://graph/summary", "graph summary").no_annotation(),
                RawResource::new("grm://docs/query-language", "query language").no_annotation(),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let text = match request.uri.as_str() {
            "grm://schema" => serde_json::to_string_pretty(&self.schema_json().await)
                .map_err(|err| McpError::internal_error(err.to_string(), None))?,
            "grm://graph/export" => {
                serde_json::to_string_pretty(&self.export_json().await.map_err(to_mcp_error)?)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?
            }
            "grm://graph/summary" => serde_json::to_string_pretty(&self.summary_json().await)
                .map_err(|err| McpError::internal_error(err.to_string(), None))?,
            "grm://docs/query-language" => compact_query_doc(),
            _ => {
                return Err(McpError::resource_not_found(
                    "resource not found",
                    Some(json!({ "uri": request.uri })),
                ));
            }
        };

        Ok(ReadResourceResult::new(vec![ResourceContents::text(
            text,
            &request.uri,
        )]))
    }
}

pub(crate) fn to_mcp_error(err: GrmError) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

fn compact_query_doc() -> String {
    QUERY_LANGUAGE_DOC
        .lines()
        .take_while(|line| !line.starts_with("## Output Design"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use rmcp::handler::server::wrapper::Parameters;
    use serde_json::json;

    use crate::{GrmMcpServer, NodeFindParams, StartupOptions};

    #[tokio::test]
    async fn imports_playground_and_finds_user() {
        let server = GrmMcpServer::new(StartupOptions {
            import_json: Some(PathBuf::from("../test-dbs/query-playground.export.json")),
            ..StartupOptions::default()
        })
        .unwrap();

        let result = server
            .grm_node_find(Parameters(NodeFindParams {
                model: "User".to_string(),
                filters: BTreeMap::from([("name".to_string(), json!("Alice Jones"))]),
            }))
            .await
            .unwrap()
            .0;

        assert_eq!(result["nodes"].as_array().unwrap().len(), 1);
    }
}
