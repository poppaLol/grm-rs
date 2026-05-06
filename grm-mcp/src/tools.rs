use std::collections::BTreeMap;
use std::io::Cursor;

use grm_rs::{CliSession, GrmError, Result as GrmResult, RuntimeNodeModel, RuntimeRelModel};
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

use crate::help::{AGENT_GUIDE, help_index, known_tools, tool_help, tool_help_index};
use crate::schema::{
    BatchEndpoint, BatchOp, BatchParams, BatchResponse, DefineEdgeParams, DefineNodeParams,
    EdgeCreateParams, EdgeDeleteParams, EdgeFindParams, EdgeUpdateParams, ExportParams, FileFormat,
    FileFormatParams, NodeCreateParams, NodeDeleteParams, NodeFindParams, NodeUpdateParams,
    PathParams, QueryParams, ToolHelpParams, json_error, parse_fields, to_object, value_map_to_raw,
};
use crate::server::GrmMcpServer;

const QUERY_LANGUAGE_DOC: &str = include_str!("../../docs/query-language-design.md");

#[tool_router(vis = "pub(crate)")]
impl GrmMcpServer {
    #[tool(description = "Return GRM agent guidance, value rules, resources, and common workflow.")]
    async fn grm_help(&self) -> Result<Json<JsonObject>, McpError> {
        Ok(Json(to_object(help_index())?))
    }

    #[tool(
        description = "Return examples and error-recovery hints for one GRM tool, e.g. {\"tool\":\"grm_node_create\"}."
    )]
    async fn grm_tool_help(
        &self,
        Parameters(params): Parameters<ToolHelpParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        let Some(help) = tool_help(&params.tool) else {
            return Err(McpError::invalid_params(
                "unknown GRM tool",
                Some(json!({
                    "tool": params.tool,
                    "known_tools": known_tools(),
                })),
            ));
        };
        Ok(Json(to_object(help)?))
    }

    #[tool(
        description = "Return the current GRM runtime schema and backend identity types. Call before graph reads/writes when model fields are unknown."
    )]
    async fn grm_schema_list(&self) -> Result<Json<JsonObject>, McpError> {
        Ok(Json(to_object(self.schema_json().await)?))
    }

    #[tool(
        description = "Apply an ordered list of structured schema/node/edge operations. Prefer this for more than 3 creates or updates."
    )]
    async fn grm_batch(
        &self,
        Parameters(params): Parameters<BatchParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        let mut state = self.state.lock().await;
        let snapshot = params.atomic.then(|| state.snapshot());
        let mut summary = BatchSummary::new(
            params.atomic,
            matches!(params.response, BatchResponse::Detailed),
            params.ops.len(),
        );
        let mut refs = BTreeMap::<String, i64>::new();

        for (index, op) in params.ops.into_iter().enumerate() {
            if op.is_delete() && !params.allow_deletes {
                summary.record_error(
                    index,
                    format!("{} requires allow_deletes=true on grm_batch", op.op_name()),
                );
                if params.atomic {
                    if let Some(snapshot) = snapshot {
                        state.restore(snapshot);
                    }
                    summary.applied = false;
                    return Ok(Json(to_object(summary.into_value())?));
                }
                continue;
            }

            let result = apply_batch_op(&mut state, &mut refs, op).await;
            match result {
                Ok(applied) => summary.record(applied),
                Err(err) => {
                    summary.record_error(index, err.to_string());
                    if params.atomic {
                        if let Some(snapshot) = snapshot {
                            state.restore(snapshot);
                        }
                        summary.applied = false;
                        return Ok(Json(to_object(summary.into_value())?));
                    }
                }
            }
        }

        if summary.applied || summary.has_successes() {
            self.persist_autocommit(&state)
                .await
                .map_err(to_mcp_error)?;
        }

        Ok(Json(to_object(summary.into_value())?))
    }

    #[tool(
        description = "Define a runtime node model. Use PascalCase model names and field types string, int, float, or bool."
    )]
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

    #[tool(
        description = "Define a runtime edge/link model between existing node models. Call grm_schema_list if endpoints are uncertain."
    )]
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

    #[tool(
        description = "Create a node for an existing runtime model. Call grm_schema_list first if required fields are unknown."
    )]
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

    #[tool(
        description = "Update an existing node by model and id. Use grm_node_find first if the id is unknown."
    )]
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

    #[tool(description = "Delete an existing node by model and id. Use grm_node_find first.")]
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

    #[tool(
        description = "Find nodes using model filters. Supports equality, comparison suffixes, limit, offset, and order."
    )]
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

    #[tool(
        description = "Create an edge between two existing node ids. Call grm_schema_list and grm_node_find if endpoints are uncertain."
    )]
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

    #[tool(description = "Update an existing edge by model and id. Use grm_edge_find first.")]
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

    #[tool(description = "Delete an existing edge by model and id. Use grm_edge_find first.")]
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

    #[tool(
        description = "Find edges using endpoint and property filters. Special filters id, from, and to only support equality."
    )]
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
        description = "Run one CLI-compatible GRM session command. Best for traversal queries; read grm://docs/query-language for syntax."
    )]
    async fn grm_query(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        let mut state = self.state.lock().await;
        let current = std::mem::take(&mut *state);
        let mut session =
            CliSession::with_state(current, Cursor::new(Vec::<u8>::new()), Vec::new());
        let result = session.handle_command(&params.command).await;
        let (next_state, _, output) = session.into_parts();
        *state = next_state;
        let should_exit = result.map_err(to_mcp_error)?;
        self.persist_autocommit(&state)
            .await
            .map_err(to_mcp_error)?;
        Ok(Json(to_object(json!({
            "command": params.command,
            "should_exit": should_exit,
            "output": String::from_utf8_lossy(&output).to_string(),
        }))?))
    }

    #[tool(description = "Save the current GRM session snapshot to a JSON or binary session file.")]
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

    #[tool(description = "Load a GRM session snapshot from a JSON or binary session file.")]
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

    #[tool(
        description = "Import a GRM interchange JSON document into an empty session. Use a fresh server if import says the session is not empty."
    )]
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
        description = "Export the current graph as GRM interchange JSON, optionally writing it to a path. Use to verify writes."
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
                RawResource::new("grm://docs/agent-guide", "agent guide").no_annotation(),
                RawResource::new("grm://docs/query-language", "query language").no_annotation(),
                RawResource::new("grm://docs/tool-help", "tool help").no_annotation(),
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
            "grm://docs/agent-guide" => AGENT_GUIDE.to_string(),
            "grm://docs/query-language" => compact_query_doc(),
            "grm://docs/tool-help" => serde_json::to_string_pretty(&tool_help_index())
                .map_err(|err| McpError::internal_error(err.to_string(), None))?,
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

struct BatchApplied {
    op: &'static str,
    model: String,
    id: Option<i64>,
    local_ref: Option<String>,
}

struct BatchSummary {
    applied: bool,
    atomic: bool,
    detailed: bool,
    operation_count: usize,
    counts: BTreeMap<String, BTreeMap<String, usize>>,
    errors: Vec<serde_json::Value>,
    ids: Vec<serde_json::Value>,
}

impl BatchSummary {
    fn new(atomic: bool, detailed: bool, operation_count: usize) -> Self {
        Self {
            applied: true,
            atomic,
            detailed,
            operation_count,
            counts: BTreeMap::new(),
            errors: Vec::new(),
            ids: Vec::new(),
        }
    }

    fn record(&mut self, applied: BatchApplied) {
        *self
            .counts
            .entry(applied.op.to_string())
            .or_default()
            .entry(applied.model.clone())
            .or_default() += 1;

        if self.detailed {
            if let Some(id) = applied.id {
                let mut value = json!({
                    "op": applied.op,
                    "model": applied.model,
                    "id": id,
                });
                if let Some(local_ref) = applied.local_ref {
                    value["ref"] = json!(local_ref);
                }
                self.ids.push(value);
            }
        }
    }

    fn record_error(&mut self, index: usize, message: String) {
        self.applied = false;
        self.errors.push(json!({
            "index": index,
            "message": message,
            "recovery": "Inspect the operation at this index, call grm_schema_list if model fields or ids are uncertain, then retry the failed operation."
        }));
    }

    fn has_successes(&self) -> bool {
        !self.counts.is_empty()
    }

    fn into_value(self) -> serde_json::Value {
        let mut value = json!({
            "applied": self.applied,
            "atomic": self.atomic,
            "operation_count": self.operation_count,
            "counts": self.counts,
            "errors": self.errors,
        });
        if self.detailed {
            value["ids"] = json!(self.ids);
        }
        value
    }
}

async fn apply_batch_op(
    state: &mut grm_rs::SessionState,
    refs: &mut BTreeMap<String, i64>,
    op: BatchOp,
) -> GrmResult<BatchApplied> {
    let op_name = op.op_name();
    match op {
        BatchOp::SchemaDefineNode(params) => {
            let model = RuntimeNodeModel::new(
                params.name.clone(),
                params.id_field,
                state.node_id_type(),
                parse_fields(params.fields)?,
            )?;
            state.register_model(model)?;
            Ok(BatchApplied {
                op: op_name,
                model: params.name,
                id: None,
                local_ref: None,
            })
        }
        BatchOp::SchemaDefineEdge(params) => {
            let model = RuntimeRelModel::new(
                params.name.clone(),
                params.from_model,
                params.to_model,
                params.id_field,
                state.rel_id_type(),
                parse_fields(params.fields)?,
            )?;
            state.register_rel_model(model)?;
            Ok(BatchApplied {
                op: op_name,
                model: params.name,
                id: None,
                local_ref: None,
            })
        }
        BatchOp::NodeCreate(params) => {
            if let Some(local_ref) = &params.local_ref {
                if refs.contains_key(local_ref) {
                    return Err(GrmError::Constraint(format!(
                        "duplicate batch ref '{local_ref}'"
                    )));
                }
            }
            let props = value_map_to_raw(params.props)?;
            let node = state.create_instance(&params.model, &props).await?;
            if let Some(local_ref) = &params.local_ref {
                refs.insert(local_ref.clone(), node.id);
            }
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(node.id),
                local_ref: params.local_ref,
            })
        }
        BatchOp::NodeUpdate(params) => {
            let props = value_map_to_raw(params.props)?;
            let node = state
                .update_node_instance(&params.model, &params.id.to_string(), &props)
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(node.id),
                local_ref: None,
            })
        }
        BatchOp::NodeDelete(params) => {
            state
                .delete_node_instance(&params.model, &params.id.to_string())
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(params.id),
                local_ref: None,
            })
        }
        BatchOp::EdgeCreate(params) => {
            let from = resolve_batch_endpoint(&params.from, refs, "from")?;
            let to = resolve_batch_endpoint(&params.to, refs, "to")?;
            let props = value_map_to_raw(params.props)?;
            let edge = state
                .create_relationship_instance(
                    &params.model,
                    &from.to_string(),
                    &to.to_string(),
                    &props,
                )
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(edge.id),
                local_ref: None,
            })
        }
        BatchOp::EdgeUpdate(params) => {
            let props = value_map_to_raw(params.props)?;
            let edge = state
                .update_relationship_instance(&params.model, &params.id.to_string(), &props)
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(edge.id),
                local_ref: None,
            })
        }
        BatchOp::EdgeDelete(params) => {
            state
                .delete_relationship_instance(&params.model, &params.id.to_string())
                .await?;
            Ok(BatchApplied {
                op: op_name,
                model: params.model,
                id: Some(params.id),
                local_ref: None,
            })
        }
    }
}

fn resolve_batch_endpoint(
    endpoint: &BatchEndpoint,
    refs: &BTreeMap<String, i64>,
    field: &str,
) -> GrmResult<i64> {
    match endpoint {
        BatchEndpoint::Id(id) => Ok(*id),
        BatchEndpoint::Ref(local_ref) => refs.get(local_ref).copied().ok_or_else(|| {
            GrmError::Constraint(format!(
                "{field} ref '{local_ref}' was not created earlier in this batch"
            ))
        }),
    }
}

fn compact_query_doc() -> String {
    QUERY_LANGUAGE_DOC
        .lines()
        .take_while(|line| !line.starts_with("## Output Design"))
        .collect::<Vec<_>>()
        .join("\n")
}
