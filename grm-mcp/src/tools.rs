use std::collections::BTreeMap;
use std::io::Cursor;

use grm_rs::{
    CliSession, DefineEdgeRequest, DefineNodeRequest, DurableOperation, EdgeCreateRequest,
    EdgeDeleteRequest, EdgeFindRequest, EdgeResponse, EdgeUpdateRequest, FieldSpec, FieldValueType,
    GraphTx, GrmError, KernelValue, Neo4jTx, NodeCreateRequest, NodeDeleteRequest, NodeFindRequest,
    NodeResponse, NodeUpdateRequest, OrderDirection, OrderSpec, PredicateOp, PropertyPredicate,
    QueryRequest, RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeRequest, RuntimeResponse,
    RuntimeValueType, SessionBatchEndpoint, SessionBatchFieldParam, SessionBatchOp,
    SessionBatchParams, SessionBatchResponse, StoredNode, StoredRel, apply_session_batch,
    client::Transaction,
    runtime::{SessionCommand, parse_command_line},
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    AnnotateAble, Implementation, JsonObject, ListResourcesResult, PaginatedRequestParams,
    RawResource, ReadResourceRequestParams, ReadResourceResult, ResourceContents,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData as McpError, Json, RoleServer, ServerHandler, tool, tool_handler, tool_router,
};
use serde_json::{Value, json};

use crate::help::{AGENT_GUIDE, help_index, known_tools, tool_help, tool_help_index};
use crate::schema::{
    BatchParams, DefineEdgeParams, DefineNodeParams, EdgeCreateParams, EdgeDeleteParams,
    EdgeFindParams, EdgeUpdateParams, ExportParams, FileFormat, FileFormatParams, NodeCreateParams,
    NodeDeleteParams, NodeFindParams, NodeUpdateParams, PathParams, QueryParams, ToolHelpParams,
    json_error, to_object, value_map_to_raw,
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
        Ok(Json(to_object(
            self.schema_json().await.map_err(to_mcp_error)?,
        )?))
    }

    #[tool(
        description = "Inspect GRM's current system index catalog. Indexes are backend-maintained derived metadata, not user-defined or durable source-of-truth data."
    )]
    async fn grm_index_list(&self) -> Result<Json<JsonObject>, McpError> {
        if let Some(err) = self.unsupported_in_service("grm_index_list") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_index_list") {
            return Err(err);
        }
        let state = self.state.lock().await;
        Ok(Json(to_object(state.index_catalog_value())?))
    }

    #[tool(
        description = "Apply an ordered list of structured schema/node/edge operation objects. Do not JSON-encode operations as strings; each ops item must be an object like {\"op\":\"node_create\",\"args\":{...}}. Prefer this for more than 3 creates or updates."
    )]
    async fn grm_batch(
        &self,
        Parameters(params): Parameters<BatchParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .batch(params.0)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_batch(params.0).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        let mut state = self.state.lock().await;
        let outcome = apply_session_batch(&mut state, params.0)
            .await
            .map_err(to_mcp_error)?;
        if outcome.should_persist {
            self.append_autocommit_ops(&state, &outcome.durable_ops)
                .await
                .map_err(to_mcp_error)?;
        }

        Ok(Json(to_object(outcome.value)?))
    }

    #[tool(
        description = "Define a runtime node model. Use PascalCase model names and field types string, int, float, or bool."
    )]
    async fn grm_schema_define_node(
        &self,
        Parameters(params): Parameters<DefineNodeParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if self.is_service() {
            let _ = self
                .service_backend()
                .map_err(to_mcp_error)?
                .define_node(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(
                self.schema_json().await.map_err(to_mcp_error)?,
            )?));
        }
        if self.is_neo4j() {
            let mut state = self.state.lock().await;
            let outcome = state
                .apply_define_node(DefineNodeRequest {
                    name: params.name,
                    id_field: params.id_field,
                    fields: field_params_to_specs(params.fields).map_err(to_mcp_error)?,
                })
                .map_err(to_mcp_error)?;
            self.append_schema_template_ops(&state, std::slice::from_ref(&outcome.durable_op))
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(state.schema_value())?));
        }

        self.with_state_mut_durable(async |state| {
            let outcome = state.apply_define_node(DefineNodeRequest {
                name: params.name,
                id_field: params.id_field,
                fields: field_params_to_specs(params.fields)?,
            })?;
            Ok((state.schema_value(), vec![outcome.durable_op]))
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
        if self.is_service() {
            let _ = self
                .service_backend()
                .map_err(to_mcp_error)?
                .define_edge(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(
                self.schema_json().await.map_err(to_mcp_error)?,
            )?));
        }
        if self.is_neo4j() {
            let mut state = self.state.lock().await;
            let outcome = state
                .apply_define_edge(DefineEdgeRequest {
                    name: params.name,
                    from_model: params.from_model,
                    to_model: params.to_model,
                    id_field: params.id_field,
                    fields: field_params_to_specs(params.fields).map_err(to_mcp_error)?,
                })
                .map_err(to_mcp_error)?;
            self.append_schema_template_ops(&state, std::slice::from_ref(&outcome.durable_op))
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(state.schema_value())?));
        }

        self.with_state_mut_durable(async |state| {
            let outcome = state.apply_define_edge(DefineEdgeRequest {
                name: params.name,
                from_model: params.from_model,
                to_model: params.to_model,
                id_field: params.id_field,
                fields: field_params_to_specs(params.fields)?,
            })?;
            Ok((state.schema_value(), vec![outcome.durable_op]))
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
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .node_create(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_node_create(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }

        self.with_state_mut_durable(async |state| {
            let outcome = state
                .apply_node_create(NodeCreateRequest {
                    model: params.model,
                    props: params.props,
                })
                .await?;
            let value = serde_json::to_value(&outcome.value).map_err(json_error)?;
            Ok((value, vec![outcome.durable_op]))
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
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .node_update(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_node_update(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        self.with_state_mut_durable(async |state| {
            let outcome = state
                .apply_node_update(NodeUpdateRequest {
                    model: params.model,
                    id: params.id,
                    props: params.props,
                })
                .await?;
            let value = serde_json::to_value(&outcome.value).map_err(json_error)?;
            Ok((value, vec![outcome.durable_op]))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Delete an existing node by model and id. Use grm_node_find first.")]
    async fn grm_node_delete(
        &self,
        Parameters(params): Parameters<NodeDeleteParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .node_delete(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_node_delete(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        self.with_state_mut_durable(async |state| {
            let outcome = state
                .apply_node_delete(NodeDeleteRequest {
                    model: params.model,
                    id: params.id,
                })
                .await?;
            Ok((
                json!({ "deleted": true, "model": outcome.value.model, "id": outcome.value.id }),
                vec![outcome.durable_op],
            ))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(
        description = "Find nodes using model filters. Supports equality, comparison suffixes, via traversal steps, end_filters, edge_filters, return, order, limit, and offset."
    )]
    async fn grm_node_find(
        &self,
        Parameters(params): Parameters<NodeFindParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .node_find(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_node_find(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }

        self.with_state_mut(false, async |state| {
            let request = params.into_node_find_request()?;
            let response = match state
                .execute_runtime(RuntimeRequest::Query(QueryRequest::NodeFind(request)))
                .await?
                .response
            {
                RuntimeResponse::Node(NodeResponse::Find(response)) => response,
                _ => {
                    return Err(GrmError::NotSupported(
                        "runtime dispatcher returned unexpected node find response",
                    ));
                }
            };
            serde_json::to_value(response).map_err(json_error)
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
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .edge_create(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_edge_create(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }

        self.with_state_mut_durable(async |state| {
            let outcome = state
                .apply_edge_create(EdgeCreateRequest {
                    model: params.model,
                    from: params.from,
                    to: params.to,
                    props: params.props,
                })
                .await?;
            let value = serde_json::to_value(&outcome.value).map_err(json_error)?;
            Ok((value, vec![outcome.durable_op]))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Update an existing edge by model and id. Use grm_edge_find first.")]
    async fn grm_edge_update(
        &self,
        Parameters(params): Parameters<EdgeUpdateParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .edge_update(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_edge_update(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        self.with_state_mut_durable(async |state| {
            let outcome = state
                .apply_edge_update(EdgeUpdateRequest {
                    model: params.model,
                    id: params.id,
                    props: params.props,
                })
                .await?;
            let value = serde_json::to_value(&outcome.value).map_err(json_error)?;
            Ok((value, vec![outcome.durable_op]))
        })
        .await
        .and_then(|value| Ok(Json(to_object(value)?)))
    }

    #[tool(description = "Delete an existing edge by model and id. Use grm_edge_find first.")]
    async fn grm_edge_delete(
        &self,
        Parameters(params): Parameters<EdgeDeleteParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .edge_delete(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_edge_delete(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        self.with_state_mut_durable(async |state| {
            let outcome = state
                .apply_edge_delete(EdgeDeleteRequest {
                    model: params.model,
                    id: params.id,
                })
                .await?;
            Ok((
                json!({ "deleted": true, "model": outcome.value.model, "id": outcome.value.id }),
                vec![outcome.durable_op],
            ))
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
        if self.is_service() {
            let value = self
                .service_backend()
                .map_err(to_mcp_error)?
                .edge_find(params)
                .await
                .map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }
        if self.is_neo4j() {
            let value = self.neo4j_edge_find(params).await.map_err(to_mcp_error)?;
            return Ok(Json(to_object(value)?));
        }

        self.with_state_mut(false, async |state| {
            let request =
                EdgeFindRequest::from_adapter_filter_values(params.model, params.filters)?;
            let response = match state
                .execute_runtime(RuntimeRequest::Query(QueryRequest::EdgeFind(request)))
                .await?
                .response
            {
                RuntimeResponse::Edge(EdgeResponse::Find(response)) => response,
                _ => {
                    return Err(GrmError::NotSupported(
                        "runtime dispatcher returned unexpected edge find response",
                    ));
                }
            };
            serde_json::to_value(response).map_err(json_error)
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
        if let Some(err) = self.unsupported_in_service("grm_query") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_query") {
            return Err(err);
        }
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

    #[tool(
        description = "Explain a CLI-compatible node.find or edge.find command and return the current logical plan as structured JSON."
    )]
    async fn grm_explain(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if let Some(err) = self.unsupported_in_service("grm_explain") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_explain") {
            return Err(err);
        }
        let state = self.state.lock().await;
        let value = match parse_introspection_command("session.explain", &params.command)? {
            SessionCommand::SessionExplainNodeFind {
                model_name, terms, ..
            } => state
                .explain_node_find_terms(&model_name, &terms)
                .map_err(to_mcp_error)?,
            SessionCommand::SessionExplainEdgeFind {
                model_name, terms, ..
            } => state
                .explain_edge_find_terms(&model_name, &terms)
                .map_err(to_mcp_error)?,
            _ => unreachable!("parse_introspection_command returns explain commands only"),
        };
        Ok(Json(to_object(value)?))
    }

    #[tool(
        description = "Profile a CLI-compatible node.find or edge.find command and return its plan, row count, and elapsed time as structured JSON."
    )]
    async fn grm_profile(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if let Some(err) = self.unsupported_in_service("grm_profile") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_profile") {
            return Err(err);
        }
        let state = self.state.lock().await;
        let value = match parse_introspection_command("session.profile", &params.command)? {
            SessionCommand::SessionProfileNodeFind {
                model_name, terms, ..
            } => state
                .profile_node_find_terms(&model_name, &terms)
                .await
                .map_err(to_mcp_error)?,
            SessionCommand::SessionProfileEdgeFind {
                model_name, terms, ..
            } => state
                .profile_edge_find_terms(&model_name, &terms)
                .map_err(to_mcp_error)?,
            _ => unreachable!("parse_introspection_command returns profile commands only"),
        };
        Ok(Json(to_object(value)?))
    }

    #[tool(description = "Save the current GRM session snapshot to a JSON or binary session file.")]
    async fn grm_save(
        &self,
        Parameters(params): Parameters<FileFormatParams>,
    ) -> Result<Json<JsonObject>, McpError> {
        if let Some(err) = self.unsupported_in_service("grm_save") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_save") {
            return Err(err);
        }
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
        if let Some(err) = self.unsupported_in_service("grm_load") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_load") {
            return Err(err);
        }
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
        if let Some(err) = self.unsupported_in_service("grm_import") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_import") {
            return Err(err);
        }
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
        if let Some(err) = self.unsupported_in_service("grm_export") {
            return Err(err);
        }
        if let Some(err) = self.unsupported_in_neo4j("grm_export") {
            return Err(err);
        }
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

impl GrmMcpServer {
    async fn neo4j_batch(&self, params: SessionBatchParams) -> grm_rs::Result<Value> {
        if !params.atomic {
            return Err(GrmError::NotSupported(
                "Neo4j grm_batch currently requires atomic=true; graph writes are committed only after every supported operation succeeds",
            ));
        }

        let mut state = self.state.lock().await;
        let mut staged = state.snapshot();
        let mut refs = BTreeMap::<String, i64>::new();
        let mut schema_ops = Vec::new();
        let mut summary = Neo4jBatchSummary::new(
            true,
            matches!(params.response, SessionBatchResponse::Detailed),
            params.ops.len(),
        );
        let mut tx = self.neo4j_client()?.transaction().await?;

        for (index, op) in params.ops.into_iter().enumerate() {
            if let Err(err) = ensure_neo4j_batch_op_supported(&op) {
                let _ = tx.rollback().await;
                return Err(GrmError::Constraint(err));
            }
            if op.is_delete() && !params.allow_deletes {
                let _ = tx.rollback().await;
                summary.record_error(
                    index,
                    format!("{} requires allow_deletes=true on grm_batch", op.op_name()),
                );
                return Ok(summary.into_value());
            }

            let op_name = op.op_name();
            let result = match op {
                SessionBatchOp::SchemaDefineNode(params) => (|| {
                    let model = RuntimeNodeModel::new(
                        params.name.clone(),
                        params.id_field,
                        staged.node_id_type(),
                        parse_batch_fields(params.fields)?,
                    )?;
                    staged.register_model(model.clone())?;
                    schema_ops.push(DurableOperation::RegisterNodeModel { model });
                    Ok(Neo4jBatchApplied {
                        op: op_name,
                        model: params.name,
                        id: None,
                        local_ref: None,
                    })
                })(),
                SessionBatchOp::SchemaDefineEdge(params) => (|| {
                    let model = RuntimeRelModel::new(
                        params.name.clone(),
                        params.from_model,
                        params.to_model,
                        params.id_field,
                        staged.rel_id_type(),
                        parse_batch_fields(params.fields)?,
                    )?;
                    staged.register_rel_model(model.clone())?;
                    schema_ops.push(DurableOperation::RegisterRelModel { model });
                    Ok(Neo4jBatchApplied {
                        op: op_name,
                        model: params.name,
                        id: None,
                        local_ref: None,
                    })
                })(),
                SessionBatchOp::NodeCreate(params) => {
                    if let Some(local_ref) = &params.local_ref {
                        if refs.contains_key(local_ref) {
                            Err(GrmError::Constraint(format!(
                                "duplicate batch ref '{local_ref}'"
                            )))
                        } else {
                            create_neo4j_batch_node(&mut tx, &staged, &mut refs, params, op_name)
                                .await
                        }
                    } else {
                        create_neo4j_batch_node(&mut tx, &staged, &mut refs, params, op_name).await
                    }
                }
                SessionBatchOp::EdgeCreate(params) => {
                    create_neo4j_batch_edge(&mut tx, &staged, &refs, params, op_name).await
                }
                SessionBatchOp::NodeUpdate(params) => {
                    update_neo4j_batch_node(&mut tx, &staged, params, op_name).await
                }
                SessionBatchOp::NodeDelete(params) => {
                    delete_neo4j_batch_node(&mut tx, &staged, params, op_name).await
                }
                SessionBatchOp::EdgeUpdate(params) => {
                    update_neo4j_batch_edge(&mut tx, &staged, params, op_name).await
                }
                SessionBatchOp::EdgeDelete(params) => {
                    delete_neo4j_batch_edge(&mut tx, &staged, params, op_name).await
                }
            };

            let applied = match result {
                Ok(applied) => applied,
                Err(err) => {
                    let _ = tx.rollback().await;
                    summary.record_error(index, err.to_string());
                    return Ok(summary.into_value());
                }
            };
            summary.record(applied);
        }

        tx.commit().await?;
        *state = staged;
        self.append_schema_template_ops(&state, &schema_ops)?;
        Ok(summary.into_value())
    }

    async fn neo4j_node_create(&self, params: NodeCreateParams) -> grm_rs::Result<Value> {
        let raw = value_map_to_raw(params.props)?;
        let (label, props) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_node_model(&params.model)
                .ok_or_else(|| missing_node_schema(&params.model))?
                .clone();
            (model.label.clone(), model.validate_instance_input(&raw)?)
        };

        let mut tx = self.neo4j_client()?.transaction().await?;
        let node = tx.tx_mut()?.create_node(vec![label], props).await?;
        tx.commit().await?;
        serde_json::to_value(node).map_err(json_error)
    }

    async fn neo4j_node_update(&self, params: NodeUpdateParams) -> grm_rs::Result<Value> {
        let raw = value_map_to_raw(params.props)?;
        let (model_name, label, props) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_node_model(&params.model)
                .ok_or_else(|| missing_node_schema(&params.model))?
                .clone();
            let props = node_update_props(&model, &raw)?;
            (model.name.clone(), model.label.clone(), props)
        };

        let mut tx = self.neo4j_client()?.transaction().await?;
        let updated = update_neo4j_node(&mut tx, params.id, &model_name, &label, props).await?;
        tx.commit().await?;
        serde_json::to_value(updated).map_err(json_error)
    }

    async fn neo4j_node_delete(&self, params: NodeDeleteParams) -> grm_rs::Result<Value> {
        let (model_name, label) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_node_model(&params.model)
                .ok_or_else(|| missing_node_schema(&params.model))?
                .clone();
            (model.name.clone(), model.label.clone())
        };

        let mut tx = self.neo4j_client()?.transaction().await?;
        delete_neo4j_node(&mut tx, params.id, &model_name, &label).await?;
        tx.commit().await?;
        Ok(json!({ "deleted": true, "model": params.model, "id": params.id }))
    }

    async fn neo4j_edge_create(&self, params: EdgeCreateParams) -> grm_rs::Result<Value> {
        let raw = value_map_to_raw(params.props)?;
        let (model, from_label, to_label, props) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_rel_model(&params.model)
                .ok_or_else(|| missing_edge_schema(&params.model))?
                .clone();
            let from_label = state
                .catalog()
                .get_node_model(&model.from_model)
                .ok_or_else(|| missing_node_schema(&model.from_model))?
                .label
                .clone();
            let to_label = state
                .catalog()
                .get_node_model(&model.to_model)
                .ok_or_else(|| missing_node_schema(&model.to_model))?
                .label
                .clone();
            let props = model.validate_instance_input(&raw)?;
            (model, from_label, to_label, props)
        };

        let mut tx = self.neo4j_client()?.transaction().await?;
        let from_node = tx
            .tx_mut()?
            .find_node_by_id(params.from)
            .await?
            .ok_or_else(|| {
                GrmError::Constraint(format!("from node '{}' was not found", params.from))
            })?;
        if !from_node.labels.iter().any(|label| label == &from_label) {
            return Err(GrmError::Constraint(format!(
                "from node '{}' does not match model '{}'",
                params.from, model.from_model
            )));
        }
        let to_node = tx
            .tx_mut()?
            .find_node_by_id(params.to)
            .await?
            .ok_or_else(|| {
                GrmError::Constraint(format!("to node '{}' was not found", params.to))
            })?;
        if !to_node.labels.iter().any(|label| label == &to_label) {
            return Err(GrmError::Constraint(format!(
                "to node '{}' does not match model '{}'",
                params.to, model.to_model
            )));
        }

        let edge = tx
            .tx_mut()?
            .create_relationship(params.from, params.to, &model.rel_type, props)
            .await?;
        tx.commit().await?;
        serde_json::to_value(edge).map_err(json_error)
    }

    async fn neo4j_edge_update(&self, params: EdgeUpdateParams) -> grm_rs::Result<Value> {
        let raw = value_map_to_raw(params.props)?;
        let (model_name, rel_type, props) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_rel_model(&params.model)
                .ok_or_else(|| missing_edge_schema(&params.model))?
                .clone();
            let props = edge_update_props(&model, &raw)?;
            (model.name.clone(), model.rel_type.clone(), props)
        };

        let mut tx = self.neo4j_client()?.transaction().await?;
        let updated = update_neo4j_edge(&mut tx, params.id, &model_name, &rel_type, props).await?;
        tx.commit().await?;
        serde_json::to_value(updated).map_err(json_error)
    }

    async fn neo4j_edge_delete(&self, params: EdgeDeleteParams) -> grm_rs::Result<Value> {
        let (model_name, rel_type) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_rel_model(&params.model)
                .ok_or_else(|| missing_edge_schema(&params.model))?
                .clone();
            (model.name.clone(), model.rel_type.clone())
        };

        let mut tx = self.neo4j_client()?.transaction().await?;
        delete_neo4j_edge(&mut tx, params.id, &model_name, &rel_type).await?;
        tx.commit().await?;
        Ok(json!({ "deleted": true, "model": params.model, "id": params.id }))
    }

    async fn neo4j_node_find(&self, params: NodeFindParams) -> grm_rs::Result<Value> {
        let (request, label, predicates, order, id_field_name) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_node_model(&params.model)
                .ok_or_else(|| missing_node_schema(&params.model))?
                .clone();
            let request = parse_neo4j_node_find_request(params, &model)?;
            reject_neo4j_node_find_traversal(&request)?;
            let predicates = typed_predicates(&request.predicates, &model.fields, &model.name)?;
            validate_node_order_fields(&request.order, &model)?;
            (
                request.clone(),
                model.label.clone(),
                predicates,
                request.order.clone(),
                model.id_field_name.clone(),
            )
        };

        let mut clauses = vec![format!("MATCH (n:{})", cypher_name(&label))];
        let mut params_json = serde_json::Map::new();
        let mut cypher_predicates = Vec::new();
        if let Some(id) = request.id {
            cypher_predicates.push("id(n) = $grm_id".to_string());
            params_json.insert("grm_id".to_string(), Value::from(id));
        }
        for (index, (predicate, value)) in predicates.into_iter().enumerate() {
            cypher_predicates.push(format!(
                "n.{} {} $p{index}",
                cypher_name(&predicate.field),
                cypher_predicate_op(predicate.op)
            ));
            params_json.insert(format!("p{index}"), value);
        }
        if !cypher_predicates.is_empty() {
            clauses.push(format!("WHERE {}", cypher_predicates.join(" AND ")));
        }
        clauses.push("RETURN n".to_string());
        if !order.is_empty() {
            let terms = order
                .into_iter()
                .map(|spec| {
                    format!(
                        "{} {}",
                        cypher_node_order_expression(&spec, &id_field_name),
                        cypher_order_direction(spec.direction)
                    )
                })
                .collect::<Vec<_>>();
            clauses.push(format!("ORDER BY {}", terms.join(", ")));
        }
        if let Some(offset) = request.offset {
            clauses.push("SKIP $grm_offset".to_string());
            params_json.insert("grm_offset".to_string(), Value::from(offset as i64));
        }
        if let Some(limit) = request.limit {
            clauses.push("LIMIT $grm_limit".to_string());
            params_json.insert("grm_limit".to_string(), Value::from(limit as i64));
        }

        let mut tx = self.neo4j_client()?.transaction().await?;
        let result = tx
            .tx_mut()?
            .execute_query(&clauses.join(" "), Value::Object(params_json))
            .await?;
        tx.commit().await?;

        let nodes = result
            .rows
            .into_iter()
            .filter_map(|row| row.values.into_values().next())
            .map(|value| match value {
                KernelValue::Node(node) => Ok(StoredNode {
                    id: node.id,
                    labels: node.labels,
                    props: node.props,
                }),
                _ => Err(GrmError::Mapping(
                    "Neo4j node find returned a non-node value".into(),
                )),
            })
            .collect::<grm_rs::Result<Vec<_>>>()?;
        Ok(json!({ "model": request.model, "nodes": nodes }))
    }

    async fn neo4j_edge_find(&self, params: EdgeFindParams) -> grm_rs::Result<Value> {
        let (request, rel_type, edge_predicates, order, id_field_name) = {
            let state = self.state.lock().await;
            let model = state
                .catalog()
                .get_rel_model(&params.model)
                .ok_or_else(|| missing_edge_schema(&params.model))?
                .clone();
            let request = parse_neo4j_edge_find_request(params, &model)?;
            let predicates = typed_predicates(&request.predicates, &model.fields, &model.name)?;
            validate_edge_order_fields(&request.order, &model)?;
            (
                request.clone(),
                model.rel_type.clone(),
                predicates,
                request.order.clone(),
                model.id_field_name.clone(),
            )
        };

        let mut clauses = vec![format!("MATCH ()-[r:{}]->()", cypher_name(&rel_type))];
        let mut params_json = serde_json::Map::new();
        let mut predicates = Vec::new();
        if let Some(id) = request.id {
            predicates.push("id(r) = $grm_id".to_string());
            params_json.insert("grm_id".to_string(), Value::from(id));
        }
        if let Some(id) = request.from {
            predicates.push("id(startNode(r)) = $from_id".to_string());
            params_json.insert("from_id".to_string(), Value::from(id));
        }
        if let Some(id) = request.to {
            predicates.push("id(endNode(r)) = $to_id".to_string());
            params_json.insert("to_id".to_string(), Value::from(id));
        }
        for (index, (predicate, value)) in edge_predicates.into_iter().enumerate() {
            predicates.push(format!(
                "r.{} {} $p{index}",
                cypher_name(&predicate.field),
                cypher_predicate_op(predicate.op)
            ));
            params_json.insert(format!("p{index}"), value);
        }
        if !predicates.is_empty() {
            clauses.push(format!("WHERE {}", predicates.join(" AND ")));
        }
        clauses.push("RETURN r".to_string());
        if !order.is_empty() {
            let terms = order
                .into_iter()
                .map(|spec| {
                    format!(
                        "{} {}",
                        cypher_edge_order_expression(&spec, &id_field_name),
                        cypher_order_direction(spec.direction)
                    )
                })
                .collect::<Vec<_>>();
            clauses.push(format!("ORDER BY {}", terms.join(", ")));
        }
        if let Some(offset) = request.offset {
            clauses.push("SKIP $grm_offset".to_string());
            params_json.insert("grm_offset".to_string(), Value::from(offset as i64));
        }
        if let Some(limit) = request.limit {
            clauses.push("LIMIT $grm_limit".to_string());
            params_json.insert("grm_limit".to_string(), Value::from(limit as i64));
        }

        let mut tx = self.neo4j_client()?.transaction().await?;
        let result = tx
            .tx_mut()?
            .execute_query(&clauses.join(" "), Value::Object(params_json))
            .await?;
        tx.commit().await?;

        let edges = result
            .rows
            .into_iter()
            .filter_map(|row| row.values.into_values().next())
            .map(|value| match value {
                KernelValue::Rel(rel) => Ok(StoredRel {
                    id: rel.id,
                    rel_type: rel.ty,
                    from: rel.from,
                    to: rel.to,
                    props: rel.props,
                }),
                _ => Err(GrmError::Mapping(
                    "Neo4j edge find returned a non-edge value".into(),
                )),
            })
            .collect::<grm_rs::Result<Vec<_>>>()?;
        Ok(json!({ "model": request.model, "edges": edges }))
    }
}

fn parse_neo4j_node_find_request(
    params: NodeFindParams,
    model: &RuntimeNodeModel,
) -> grm_rs::Result<NodeFindRequest> {
    let mut params = params;
    let mut filters = params.filters;
    let model_id = remove_model_id_alias(&mut filters, &model.id_field_name)?;
    params.filters = filters;
    let mut request = params.into_node_find_request()?;
    merge_model_id_alias(&mut request.id, model_id, "node", &model.id_field_name)?;
    Ok(request)
}

fn parse_neo4j_edge_find_request(
    params: EdgeFindParams,
    model: &RuntimeRelModel,
) -> grm_rs::Result<EdgeFindRequest> {
    let mut filters = params.filters;
    let model_id = remove_model_id_alias(&mut filters, &model.id_field_name)?;
    let mut request = EdgeFindRequest::from_adapter_filter_values(params.model, filters)?;
    merge_model_id_alias(&mut request.id, model_id, "edge", &model.id_field_name)?;
    Ok(request)
}

fn remove_model_id_alias(
    filters: &mut BTreeMap<String, Value>,
    id_field_name: &str,
) -> grm_rs::Result<Option<i64>> {
    if id_field_name == "id" {
        return Ok(None);
    }
    let Some(value) = filters.remove(id_field_name) else {
        return Ok(None);
    };
    let raw = value_map_to_raw(BTreeMap::from([(id_field_name.to_string(), value)]))?;
    parse_named_id_filter(&raw, id_field_name)
}

fn merge_model_id_alias(
    request_id: &mut Option<i64>,
    model_id: Option<i64>,
    subject: &str,
    id_field_name: &str,
) -> grm_rs::Result<()> {
    match (*request_id, model_id) {
        (Some(left), Some(right)) if left != right => Err(GrmError::Constraint(format!(
            "conflicting {subject} id filters 'id' and '{id_field_name}'"
        ))),
        (None, Some(id)) => {
            *request_id = Some(id);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn reject_neo4j_node_find_traversal(request: &NodeFindRequest) -> grm_rs::Result<()> {
    if request.traversals.is_empty()
        && request.end_predicates.is_empty()
        && request.edge_predicates.is_empty()
        && request.return_mode.is_none()
    {
        return Ok(());
    }
    Err(GrmError::NotSupported(
        "Neo4j MCP mode supports simple grm_node_find only; traversal queries are not supported yet",
    ))
}

fn typed_predicates(
    predicates: &[PropertyPredicate],
    fields: &[RuntimeField],
    model_name: &str,
) -> grm_rs::Result<Vec<(PropertyPredicate, Value)>> {
    let mut typed = Vec::new();
    for predicate in predicates {
        let Some(field) = fields.iter().find(|field| field.name == predicate.field) else {
            return Err(GrmError::Constraint(format!(
                "unknown field '{}' for model '{model_name}'",
                predicate.field
            )));
        };
        if predicate.op == PredicateOp::Contains
            && !matches!(field.value_type, RuntimeValueType::String)
        {
            return Err(GrmError::Constraint(format!(
                "contains filter '{}' requires a string field",
                predicate.field
            )));
        }
        let raw = value_map_to_raw(BTreeMap::from([(
            predicate.field.clone(),
            predicate.value.clone(),
        )]))?;
        let raw_value = raw.get(&predicate.field).ok_or_else(|| {
            GrmError::Mapping(format!(
                "missing parsed value for field '{}'",
                predicate.field
            ))
        })?;
        typed.push((predicate.clone(), field.value_type.parse_value(raw_value)?));
    }
    Ok(typed)
}

fn validate_node_order_fields(order: &[OrderSpec], model: &RuntimeNodeModel) -> grm_rs::Result<()> {
    for spec in order {
        if spec.field == "id" || spec.field == model.id_field_name {
            continue;
        }
        if model.field(&spec.field).is_none() {
            return Err(GrmError::Constraint(format!(
                "unknown order field '{}' for model '{}'",
                spec.field, model.name
            )));
        }
    }
    Ok(())
}

fn validate_edge_order_fields(order: &[OrderSpec], model: &RuntimeRelModel) -> grm_rs::Result<()> {
    for spec in order {
        if spec.field == "id"
            || spec.field == model.id_field_name
            || spec.field == "from"
            || spec.field == "to"
        {
            continue;
        }
        if model.field(&spec.field).is_none() {
            return Err(GrmError::Constraint(format!(
                "unknown order field '{}' for link '{}'",
                spec.field, model.name
            )));
        }
    }
    Ok(())
}

fn cypher_node_order_expression(spec: &OrderSpec, id_field_name: &str) -> String {
    if spec.field == "id" || spec.field == id_field_name {
        "id(n)".to_string()
    } else {
        format!("n.{}", cypher_name(&spec.field))
    }
}

fn cypher_edge_order_expression(spec: &OrderSpec, id_field_name: &str) -> String {
    match spec.field.as_str() {
        "id" => "id(r)".to_string(),
        "from" => "id(startNode(r))".to_string(),
        "to" => "id(endNode(r))".to_string(),
        field if field == id_field_name => "id(r)".to_string(),
        _ => format!("r.{}", cypher_name(&spec.field)),
    }
}

fn cypher_predicate_op(op: PredicateOp) -> &'static str {
    match op {
        PredicateOp::Eq => "=",
        PredicateOp::Ne => "<>",
        PredicateOp::Gt => ">",
        PredicateOp::Ge => ">=",
        PredicateOp::Lt => "<",
        PredicateOp::Le => "<=",
        PredicateOp::Contains => "CONTAINS",
    }
}

fn cypher_order_direction(direction: OrderDirection) -> &'static str {
    match direction {
        OrderDirection::Asc => "ASC",
        OrderDirection::Desc => "DESC",
    }
}

fn parse_named_id_filter(raw: &BTreeMap<String, String>, key: &str) -> grm_rs::Result<Option<i64>> {
    raw.get(key)
        .map(|value| {
            value.parse::<i64>().map_err(|_| {
                GrmError::Constraint(format!("{key} filter must be an integer backend id"))
            })
        })
        .transpose()
}

struct Neo4jBatchApplied {
    op: &'static str,
    model: String,
    id: Option<i64>,
    local_ref: Option<String>,
}

struct Neo4jBatchSummary {
    applied: bool,
    atomic: bool,
    detailed: bool,
    operation_count: usize,
    counts: BTreeMap<String, BTreeMap<String, usize>>,
    errors: Vec<Value>,
    ids: Vec<Value>,
}

impl Neo4jBatchSummary {
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

    fn record(&mut self, applied: Neo4jBatchApplied) {
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
            "recovery": "Call grm_schema_list, define or reconstruct session-local runtime schema first, then retry the Neo4j batch."
        }));
    }

    fn into_value(self) -> Value {
        let mut value = json!({
            "applied": self.applied,
            "atomic": self.atomic,
            "operation_count": self.operation_count,
            "counts": self.counts,
            "errors": self.errors,
            "backend": {
                "mode": "neo4j",
                "atomicity": "Neo4j graph writes are committed in one transaction after all supported operations succeed; session-local schema metadata is staged and installed after commit."
            }
        });
        if self.detailed {
            value["ids"] = json!(self.ids);
        }
        value
    }
}

fn ensure_neo4j_batch_op_supported(op: &SessionBatchOp) -> Result<(), String> {
    match op {
        SessionBatchOp::SchemaDefineNode(_)
        | SessionBatchOp::SchemaDefineEdge(_)
        | SessionBatchOp::NodeCreate(_)
        | SessionBatchOp::NodeUpdate(_)
        | SessionBatchOp::NodeDelete(_)
        | SessionBatchOp::EdgeCreate(_)
        | SessionBatchOp::EdgeUpdate(_)
        | SessionBatchOp::EdgeDelete(_) => Ok(()),
    }
}

async fn update_neo4j_batch_node(
    tx: &mut Transaction<Neo4jTx>,
    state: &grm_rs::SessionState,
    params: grm_rs::SessionBatchNodeUpdateParams,
    op_name: &'static str,
) -> grm_rs::Result<Neo4jBatchApplied> {
    let raw = value_map_to_raw(params.props)?;
    let model = state
        .catalog()
        .get_node_model(&params.model)
        .ok_or_else(|| missing_node_schema(&params.model))?
        .clone();
    let props = node_update_props(&model, &raw)?;
    let node = update_neo4j_node(tx, params.id, &model.name, &model.label, props).await?;
    Ok(Neo4jBatchApplied {
        op: op_name,
        model: params.model,
        id: Some(node.id),
        local_ref: None,
    })
}

async fn delete_neo4j_batch_node(
    tx: &mut Transaction<Neo4jTx>,
    state: &grm_rs::SessionState,
    params: grm_rs::SessionBatchNodeDeleteParams,
    op_name: &'static str,
) -> grm_rs::Result<Neo4jBatchApplied> {
    let model = state
        .catalog()
        .get_node_model(&params.model)
        .ok_or_else(|| missing_node_schema(&params.model))?
        .clone();
    delete_neo4j_node(tx, params.id, &model.name, &model.label).await?;
    Ok(Neo4jBatchApplied {
        op: op_name,
        model: params.model,
        id: Some(params.id),
        local_ref: None,
    })
}

async fn update_neo4j_batch_edge(
    tx: &mut Transaction<Neo4jTx>,
    state: &grm_rs::SessionState,
    params: grm_rs::SessionBatchEdgeUpdateParams,
    op_name: &'static str,
) -> grm_rs::Result<Neo4jBatchApplied> {
    let raw = value_map_to_raw(params.props)?;
    let model = state
        .catalog()
        .get_rel_model(&params.model)
        .ok_or_else(|| missing_edge_schema(&params.model))?
        .clone();
    let props = edge_update_props(&model, &raw)?;
    let edge = update_neo4j_edge(tx, params.id, &model.name, &model.rel_type, props).await?;
    Ok(Neo4jBatchApplied {
        op: op_name,
        model: params.model,
        id: Some(edge.id),
        local_ref: None,
    })
}

async fn delete_neo4j_batch_edge(
    tx: &mut Transaction<Neo4jTx>,
    state: &grm_rs::SessionState,
    params: grm_rs::SessionBatchEdgeDeleteParams,
    op_name: &'static str,
) -> grm_rs::Result<Neo4jBatchApplied> {
    let model = state
        .catalog()
        .get_rel_model(&params.model)
        .ok_or_else(|| missing_edge_schema(&params.model))?
        .clone();
    delete_neo4j_edge(tx, params.id, &model.name, &model.rel_type).await?;
    Ok(Neo4jBatchApplied {
        op: op_name,
        model: params.model,
        id: Some(params.id),
        local_ref: None,
    })
}

async fn update_neo4j_node(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model_name: &str,
    label: &str,
    props: BTreeMap<String, Value>,
) -> grm_rs::Result<StoredNode> {
    let existing = tx
        .tx_mut()?
        .find_node_by_id(id)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("node '{id}' was not found")))?;
    if !existing.labels.iter().any(|candidate| candidate == label) {
        return Err(GrmError::Constraint(format!(
            "node '{id}' does not match model '{model_name}'"
        )));
    }
    tx.tx_mut()?
        .update_node(id, props)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("node '{id}' was not found")))
}

async fn delete_neo4j_node(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model_name: &str,
    label: &str,
) -> grm_rs::Result<()> {
    let existing = tx
        .tx_mut()?
        .find_node_by_id(id)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("node '{id}' was not found")))?;
    if !existing.labels.iter().any(|candidate| candidate == label) {
        return Err(GrmError::Constraint(format!(
            "node '{id}' does not match model '{model_name}'"
        )));
    }
    tx.tx_mut()?.delete_node(id).await
}

async fn update_neo4j_edge(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model_name: &str,
    rel_type: &str,
    props: BTreeMap<String, Value>,
) -> grm_rs::Result<StoredRel> {
    find_neo4j_edge(tx, id, rel_type)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("edge '{id}' was not found")))?;
    tx.tx_mut()?
        .update_relationship(id, props)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("edge '{id}' was not found")))
        .and_then(|edge| {
            if edge.rel_type == rel_type {
                Ok(edge)
            } else {
                Err(GrmError::Constraint(format!(
                    "edge '{id}' does not match model '{model_name}'"
                )))
            }
        })
}

async fn delete_neo4j_edge(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model_name: &str,
    rel_type: &str,
) -> grm_rs::Result<()> {
    find_neo4j_edge(tx, id, rel_type).await?.ok_or_else(|| {
        GrmError::Constraint(format!(
            "edge '{id}' was not found for model '{model_name}'"
        ))
    })?;
    tx.tx_mut()?.delete_relationship(id).await
}

async fn find_neo4j_edge(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    rel_type: &str,
) -> grm_rs::Result<Option<StoredRel>> {
    let result = tx
        .tx_mut()?
        .execute_query(
            &format!(
                "MATCH ()-[r:{}]->() WHERE id(r) = $grm_id RETURN r",
                cypher_name(rel_type)
            ),
            json!({ "grm_id": id }),
        )
        .await?;
    result
        .rows
        .into_iter()
        .next()
        .and_then(|row| row.values.into_values().next())
        .map(|value| match value {
            KernelValue::Rel(rel) => Ok(StoredRel {
                id: rel.id,
                rel_type: rel.ty,
                from: rel.from,
                to: rel.to,
                props: rel.props,
            }),
            _ => Err(GrmError::Mapping(
                "Neo4j edge lookup returned a non-edge value".into(),
            )),
        })
        .transpose()
}

fn node_update_props(
    model: &RuntimeNodeModel,
    raw: &BTreeMap<String, String>,
) -> grm_rs::Result<BTreeMap<String, Value>> {
    model_update_props(&model.fields, &model.name, raw, &[&model.id_field_name])
}

fn edge_update_props(
    model: &RuntimeRelModel,
    raw: &BTreeMap<String, String>,
) -> grm_rs::Result<BTreeMap<String, Value>> {
    model_update_props(
        &model.fields,
        &model.name,
        raw,
        &[&model.id_field_name, "from", "to"],
    )
}

fn model_update_props(
    fields: &[RuntimeField],
    model_name: &str,
    raw: &BTreeMap<String, String>,
    special_keys: &[&str],
) -> grm_rs::Result<BTreeMap<String, Value>> {
    let mut parsed = BTreeMap::new();
    for (key, value) in raw {
        if key == "id" || special_keys.iter().any(|special| key == special) {
            continue;
        }
        let Some(field) = fields.iter().find(|field| field.name == *key) else {
            return Err(GrmError::Constraint(format!(
                "unknown field '{key}' for model '{model_name}'"
            )));
        };
        parsed.insert(key.clone(), field.value_type.parse_value(value)?);
    }
    Ok(parsed)
}

async fn create_neo4j_batch_node(
    tx: &mut Transaction<Neo4jTx>,
    state: &grm_rs::SessionState,
    refs: &mut BTreeMap<String, i64>,
    params: grm_rs::SessionBatchNodeCreateParams,
    op_name: &'static str,
) -> grm_rs::Result<Neo4jBatchApplied> {
    let raw = value_map_to_raw(params.props)?;
    let model = state
        .catalog()
        .get_node_model(&params.model)
        .ok_or_else(|| missing_node_schema(&params.model))?
        .clone();
    let props = model.validate_instance_input(&raw)?;
    let node = tx
        .tx_mut()?
        .create_node(vec![model.label.clone()], props)
        .await?;
    if let Some(local_ref) = &params.local_ref {
        refs.insert(local_ref.clone(), node.id);
    }
    Ok(Neo4jBatchApplied {
        op: op_name,
        model: params.model,
        id: Some(node.id),
        local_ref: params.local_ref,
    })
}

async fn create_neo4j_batch_edge(
    tx: &mut Transaction<Neo4jTx>,
    state: &grm_rs::SessionState,
    refs: &BTreeMap<String, i64>,
    params: grm_rs::SessionBatchEdgeCreateParams,
    op_name: &'static str,
) -> grm_rs::Result<Neo4jBatchApplied> {
    let from = resolve_neo4j_batch_endpoint(&params.from, refs, "from")?;
    let to = resolve_neo4j_batch_endpoint(&params.to, refs, "to")?;
    let raw = value_map_to_raw(params.props)?;
    let (model, from_label, to_label, props) =
        validated_neo4j_edge_create(state, &params.model, raw)?;

    let from_node = tx
        .tx_mut()?
        .find_node_by_id(from)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("from node '{from}' was not found")))?;
    if !from_node.labels.iter().any(|label| label == &from_label) {
        return Err(GrmError::Constraint(format!(
            "from node '{from}' does not match model '{}'",
            model.from_model
        )));
    }
    let to_node = tx
        .tx_mut()?
        .find_node_by_id(to)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("to node '{to}' was not found")))?;
    if !to_node.labels.iter().any(|label| label == &to_label) {
        return Err(GrmError::Constraint(format!(
            "to node '{to}' does not match model '{}'",
            model.to_model
        )));
    }

    let edge = tx
        .tx_mut()?
        .create_relationship(from, to, &model.rel_type, props)
        .await?;
    Ok(Neo4jBatchApplied {
        op: op_name,
        model: params.model,
        id: Some(edge.id),
        local_ref: None,
    })
}

fn parse_batch_fields(fields: Vec<SessionBatchFieldParam>) -> grm_rs::Result<Vec<RuntimeField>> {
    fields
        .into_iter()
        .map(|field| {
            let value_type =
                RuntimeValueType::parse_keyword(&field.value_type).ok_or_else(|| {
                    GrmError::Constraint(format!(
                        "unsupported field type '{}', expected one of: string, int, float, bool",
                        field.value_type
                    ))
                })?;
            Ok(RuntimeField {
                name: field.name,
                value_type,
                required: field.required,
            })
        })
        .collect()
}

fn field_params_to_specs(fields: Vec<crate::schema::FieldParam>) -> grm_rs::Result<Vec<FieldSpec>> {
    fields
        .into_iter()
        .map(|field| {
            let value_type = field_value_type(&field.value_type).ok_or_else(|| {
                GrmError::Constraint(format!(
                    "unsupported field type '{}', expected one of: string, int, float, bool",
                    field.value_type
                ))
            })?;
            Ok(FieldSpec {
                name: field.name,
                value_type,
                required: field.required,
            })
        })
        .collect()
}

fn field_value_type(raw: &str) -> Option<FieldValueType> {
    match raw {
        "string" => Some(FieldValueType::String),
        "int" => Some(FieldValueType::Int),
        "float" => Some(FieldValueType::Float),
        "bool" => Some(FieldValueType::Bool),
        _ => None,
    }
}

fn resolve_neo4j_batch_endpoint(
    endpoint: &SessionBatchEndpoint,
    refs: &BTreeMap<String, i64>,
    field: &str,
) -> grm_rs::Result<i64> {
    match endpoint {
        SessionBatchEndpoint::Id(id) => Ok(*id),
        SessionBatchEndpoint::Ref(local_ref) => refs.get(local_ref).copied().ok_or_else(|| {
            GrmError::Constraint(format!(
                "{field} ref '{local_ref}' was not created earlier in this batch"
            ))
        }),
    }
}

fn validated_neo4j_edge_create(
    state: &grm_rs::SessionState,
    model_name: &str,
    raw: BTreeMap<String, String>,
) -> grm_rs::Result<(RuntimeRelModel, String, String, BTreeMap<String, Value>)> {
    let model = state
        .catalog()
        .get_rel_model(model_name)
        .ok_or_else(|| missing_edge_schema(model_name))?
        .clone();
    let from_label = state
        .catalog()
        .get_node_model(&model.from_model)
        .ok_or_else(|| missing_node_schema(&model.from_model))?
        .label
        .clone();
    let to_label = state
        .catalog()
        .get_node_model(&model.to_model)
        .ok_or_else(|| missing_node_schema(&model.to_model))?
        .label
        .clone();
    let props = model.validate_instance_input(&raw)?;
    Ok((model, from_label, to_label, props))
}

fn missing_node_schema(model: &str) -> GrmError {
    GrmError::Constraint(format!(
        "node model '{model}' is not registered in the session-local runtime schema; call grm_schema_list and define schema first with grm_schema_define_node or grm_batch schema_define_node before creating or finding typed Neo4j data"
    ))
}

fn missing_edge_schema(model: &str) -> GrmError {
    GrmError::Constraint(format!(
        "edge model '{model}' is not registered in the session-local runtime schema; call grm_schema_list and define schema first with grm_schema_define_edge or grm_batch schema_define_edge before creating or finding typed Neo4j data"
    ))
}

fn cypher_name(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

#[tool_handler]
impl ServerHandler for GrmMcpServer {
    fn get_info(&self) -> ServerInfo {
        let instructions = if self.is_service() {
            "Use GRM tools against the configured gRPC workspace service. On startup call grm_schema_list, then inspect grm://backend/status. gRPC MCP mode supports schema define/list, grm_batch for schema/node/edge create/update/delete, node_create, node_update, node_delete, edge_create, edge_update, edge_delete, traversal-capable node_find for node results, and edge_find through ExecuteWorkspace. Direct service RPC families, import/export, explain/profile, free-form query parity, and node.find return=edge results are not supported yet."
        } else if self.is_neo4j() {
            "Use GRM tools to inspect session-local runtime schema and write supported schema-aware operations directly to Neo4j. On startup call grm_schema_list, then inspect grm://backend/status; if schema_template_loaded is true, verify the recovered models before writing. If schema_template_persistence_enabled is true and schema_template_loaded is false, this server started with fresh local schema memory. If runtime schema is empty, ask whether to define or reconstruct schema before grm_batch writes. Neo4j mode supports schema define/list, grm_batch for schema/node/edge create/update/delete, node_create, node_update, node_delete, edge_create, edge_update, edge_delete, and simple node/edge find."
        } else {
            "Use GRM tools to inspect and mutate the local runtime graph session. Prefer structured tools over raw CLI commands when possible."
        };
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(instructions)
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
                RawResource::new("grm://backend/status", "backend status").no_annotation(),
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
            "grm://schema" => {
                serde_json::to_string_pretty(&self.schema_json().await.map_err(to_mcp_error)?)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?
            }
            "grm://graph/export" => {
                if let Some(err) = self.unsupported_in_service("grm://graph/export") {
                    return Err(err);
                }
                if let Some(err) = self.unsupported_in_neo4j("grm://graph/export") {
                    return Err(err);
                }
                serde_json::to_string_pretty(&self.export_json().await.map_err(to_mcp_error)?)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?
            }
            "grm://graph/summary" => {
                if let Some(err) = self.unsupported_in_service("grm://graph/summary") {
                    return Err(err);
                }
                if let Some(err) = self.unsupported_in_neo4j("grm://graph/summary") {
                    return Err(err);
                }
                serde_json::to_string_pretty(&self.summary_json().await)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?
            }
            "grm://backend/status" => {
                serde_json::to_string_pretty(&self.backend_status_json().await)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?
            }
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

fn parse_introspection_command(kind: &str, command: &str) -> Result<SessionCommand, McpError> {
    let command = command.trim();
    let opposite_kind = match kind {
        "session.explain" => "session.profile",
        "session.profile" => "session.explain",
        _ => unreachable!("only explain/profile introspection kinds are supported"),
    };
    if command == opposite_kind || command.starts_with(&format!("{opposite_kind} ")) {
        return Err(McpError::invalid_params(
            format!("expected {kind} command, got {opposite_kind}"),
            Some(json!({ "command": command })),
        ));
    }

    let command = command.strip_prefix(&format!("{kind} ")).unwrap_or(command);
    let parsed = parse_command_line(&format!("{kind} {command}")).map_err(to_mcp_error)?;

    match (&parsed, kind) {
        (SessionCommand::SessionExplainNodeFind { .. }, "session.explain")
        | (SessionCommand::SessionExplainEdgeFind { .. }, "session.explain")
        | (SessionCommand::SessionProfileNodeFind { .. }, "session.profile")
        | (SessionCommand::SessionProfileEdgeFind { .. }, "session.profile") => Ok(parsed),
        _ => Err(McpError::invalid_params(
            "expected command to be node.find <ModelName> [terms...] or edge.find <LinkName> [terms...]",
            Some(json!({ "command": command })),
        )),
    }
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
    use super::*;
    use grm_rs::BackendIdType;

    fn user_model() -> RuntimeNodeModel {
        RuntimeNodeModel::new(
            "User",
            "userId",
            BackendIdType::Int64,
            vec![
                RuntimeField {
                    name: "name".into(),
                    value_type: RuntimeValueType::String,
                    required: true,
                },
                RuntimeField {
                    name: "age".into(),
                    value_type: RuntimeValueType::Int,
                    required: true,
                },
            ],
        )
        .unwrap()
    }

    fn authored_model() -> RuntimeRelModel {
        RuntimeRelModel::new(
            "Authored",
            "User",
            "Post",
            "authoredId",
            BackendIdType::Int64,
            vec![RuntimeField {
                name: "year".into(),
                value_type: RuntimeValueType::Int,
                required: true,
            }],
        )
        .unwrap()
    }

    #[test]
    fn neo4j_node_find_parses_adapter_controls_as_request_fields() {
        let request = parse_neo4j_node_find_request(
            NodeFindParams {
                model: "User".into(),
                filters: BTreeMap::from([
                    ("age>".into(), json!(35)),
                    ("order".into(), json!("age:asc")),
                    ("limit".into(), json!(1)),
                    ("userId".into(), json!(7)),
                ]),
                via: Vec::new(),
                end_filters: BTreeMap::new(),
                edge_filters: BTreeMap::new(),
                return_mode: None,
                order: None,
                limit: None,
                offset: None,
            },
            &user_model(),
        )
        .unwrap();

        assert_eq!(request.id, Some(7));
        assert_eq!(request.limit, Some(1));
        assert_eq!(request.order[0].field, "age");
        assert_eq!(request.predicates[0].field, "age");
        assert_eq!(request.predicates[0].op, PredicateOp::Gt);
    }

    #[test]
    fn neo4j_find_rejects_unknown_predicate_and_order_fields() {
        let model = user_model();
        let request = NodeFindRequest::from_adapter_filter_values(
            "User",
            BTreeMap::from([("limit".into(), json!(1))]),
        )
        .unwrap();
        assert!(typed_predicates(&request.predicates, &model.fields, &model.name).is_ok());

        let bad_order = NodeFindRequest::from_adapter_filter_values(
            "User",
            BTreeMap::from([("order".into(), json!("missing:asc"))]),
        )
        .unwrap();
        let err = validate_node_order_fields(&bad_order.order, &model).unwrap_err();
        assert!(err.to_string().contains("unknown order field 'missing'"));
    }

    #[test]
    fn neo4j_order_validation_matches_runtime_special_fields() {
        let node_model = user_model();
        let node_order = NodeFindRequest::from_adapter_filter_values(
            "User",
            BTreeMap::from([("order".into(), json!("id:asc,userId:desc,name:asc"))]),
        )
        .unwrap();
        validate_node_order_fields(&node_order.order, &node_model).unwrap();
        assert_eq!(
            cypher_node_order_expression(&node_order.order[0], &node_model.id_field_name),
            "id(n)"
        );
        assert_eq!(
            cypher_node_order_expression(&node_order.order[1], &node_model.id_field_name),
            "id(n)"
        );

        let edge_model = authored_model();
        let edge_order = EdgeFindRequest::from_adapter_filter_values(
            "Authored",
            BTreeMap::from([(
                "order".into(),
                json!("id:asc,authoredId:desc,from:asc,to:desc,year:asc"),
            )]),
        )
        .unwrap();
        validate_edge_order_fields(&edge_order.order, &edge_model).unwrap();
        assert_eq!(
            cypher_edge_order_expression(&edge_order.order[0], &edge_model.id_field_name),
            "id(r)"
        );
        assert_eq!(
            cypher_edge_order_expression(&edge_order.order[1], &edge_model.id_field_name),
            "id(r)"
        );
        assert_eq!(
            cypher_edge_order_expression(&edge_order.order[2], &edge_model.id_field_name),
            "id(startNode(r))"
        );
        assert_eq!(
            cypher_edge_order_expression(&edge_order.order[3], &edge_model.id_field_name),
            "id(endNode(r))"
        );
    }
}
