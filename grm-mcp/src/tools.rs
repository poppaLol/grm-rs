use std::collections::BTreeMap;
use std::io::Cursor;

use grm_rs::{
    CliSession, DefineEdgeRequest, DefineNodeRequest, EdgeCreateRequest, EdgeDeleteRequest,
    EdgeFindRequest, EdgeResponse, EdgeUpdateRequest, FieldSpec, FieldValueType, GrmError,
    NodeCreateRequest, NodeDeleteRequest, NodeFindRequest, NodeResponse, NodeUpdateRequest,
    QueryRequest, RuntimeNodeModel, RuntimeRelModel, RuntimeRequest, RuntimeResponse,
    SessionBatchParams, apply_neo4j_batch, apply_session_batch, neo4j_edge_create,
    neo4j_edge_delete, neo4j_edge_find, neo4j_edge_update, neo4j_node_create, neo4j_node_delete,
    neo4j_node_find, neo4j_node_update,
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
        description = "Checkpoint Neo4j session-local runtime schema memory into the configured GRM_SCHEMA_TEMPLATE base file and clear its append log. This does not modify Neo4j graph data."
    )]
    async fn grm_schema_checkpoint(&self) -> Result<Json<JsonObject>, McpError> {
        if !self.is_neo4j() {
            return Err(McpError::internal_error(
                "grm_schema_checkpoint is only supported in Neo4j MCP mode",
                None,
            ));
        }

        let _schema_write = self.neo4j_schema_write.lock().await;
        let state = self.state.lock().await;
        let value = self
            .checkpoint_schema_template(&state)
            .map_err(to_mcp_error)?;
        Ok(Json(to_object(value)?))
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
            let _schema_write = self.neo4j_schema_write.lock().await;
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
            let _schema_write = self.neo4j_schema_write.lock().await;
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
        if self.is_service() {
            let value = match parse_introspection_command("session.explain", &params.command)? {
                SessionCommand::SessionExplainNodeFind {
                    model_name, terms, ..
                } => self
                    .service_backend()
                    .map_err(to_mcp_error)?
                    .explain_node_find_terms(&model_name, &terms)
                    .await
                    .map_err(to_mcp_error)?,
                SessionCommand::SessionExplainEdgeFind {
                    model_name, terms, ..
                } => self
                    .service_backend()
                    .map_err(to_mcp_error)?
                    .explain_edge_find_terms(&model_name, &terms)
                    .await
                    .map_err(to_mcp_error)?,
                _ => unreachable!("parse_introspection_command returns explain commands only"),
            };
            return Ok(Json(to_object(value)?));
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
        if self.is_service() {
            let value = match parse_introspection_command("session.profile", &params.command)? {
                SessionCommand::SessionProfileNodeFind {
                    model_name, terms, ..
                } => self
                    .service_backend()
                    .map_err(to_mcp_error)?
                    .profile_node_find_terms(&model_name, &terms)
                    .await
                    .map_err(to_mcp_error)?,
                SessionCommand::SessionProfileEdgeFind {
                    model_name, terms, ..
                } => self
                    .service_backend()
                    .map_err(to_mcp_error)?
                    .profile_edge_find_terms(&model_name, &terms)
                    .await
                    .map_err(to_mcp_error)?,
                _ => unreachable!("parse_introspection_command returns profile commands only"),
            };
            return Ok(Json(to_object(value)?));
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
        let _schema_write = self.neo4j_schema_write.lock().await;
        let mut staged = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let outcome = apply_neo4j_batch(self.neo4j_client()?, &mut staged, params).await?;
        if outcome.value["applied"] == json!(true) {
            let mut state = self.state.lock().await;
            *state = staged;
            self.append_schema_template_ops(&state, &outcome.schema_ops)?;
        }
        Ok(with_neo4j_batch_backend(outcome.value))
    }

    async fn neo4j_node_create(&self, params: NodeCreateParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let node = neo4j_node_create(
            self.neo4j_client()?,
            &state,
            NodeCreateRequest {
                model: params.model,
                props: params.props,
            },
        )
        .await?;
        serde_json::to_value(node).map_err(json_error)
    }

    async fn neo4j_node_update(&self, params: NodeUpdateParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let updated = neo4j_node_update(
            self.neo4j_client()?,
            &state,
            NodeUpdateRequest {
                model: params.model,
                id: params.id,
                props: params.props,
            },
        )
        .await?;
        serde_json::to_value(updated).map_err(json_error)
    }

    async fn neo4j_node_delete(&self, params: NodeDeleteParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        neo4j_node_delete(
            self.neo4j_client()?,
            &state,
            NodeDeleteRequest {
                model: params.model.clone(),
                id: params.id,
            },
        )
        .await?;
        Ok(json!({ "deleted": true, "model": params.model, "id": params.id }))
    }

    async fn neo4j_edge_create(&self, params: EdgeCreateParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let edge = neo4j_edge_create(
            self.neo4j_client()?,
            &state,
            EdgeCreateRequest {
                model: params.model,
                from: params.from,
                to: params.to,
                props: params.props,
            },
        )
        .await?;
        serde_json::to_value(edge).map_err(json_error)
    }

    async fn neo4j_edge_update(&self, params: EdgeUpdateParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let updated = neo4j_edge_update(
            self.neo4j_client()?,
            &state,
            EdgeUpdateRequest {
                model: params.model,
                id: params.id,
                props: params.props,
            },
        )
        .await?;
        serde_json::to_value(updated).map_err(json_error)
    }

    async fn neo4j_edge_delete(&self, params: EdgeDeleteParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        neo4j_edge_delete(
            self.neo4j_client()?,
            &state,
            EdgeDeleteRequest {
                model: params.model.clone(),
                id: params.id,
            },
        )
        .await?;
        Ok(json!({ "deleted": true, "model": params.model, "id": params.id }))
    }

    async fn neo4j_node_find(&self, params: NodeFindParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let request = {
            let model = state
                .catalog()
                .get_node_model(&params.model)
                .ok_or_else(|| missing_node_schema(&params.model))?;
            parse_neo4j_node_find_request(params, model)?
        };
        let model_name = request.model.clone();
        let nodes = neo4j_node_find(self.neo4j_client()?, &state, request).await?;
        Ok(json!({ "model": model_name, "nodes": nodes }))
    }

    async fn neo4j_edge_find(&self, params: EdgeFindParams) -> grm_rs::Result<Value> {
        let state = {
            let state = self.state.lock().await;
            state.snapshot()
        };
        let request = {
            let model = state
                .catalog()
                .get_rel_model(&params.model)
                .ok_or_else(|| missing_edge_schema(&params.model))?;
            parse_neo4j_edge_find_request(params, model)?
        };
        let model_name = request.model.clone();
        let edges = neo4j_edge_find(self.neo4j_client()?, &state, request).await?;
        Ok(json!({ "model": model_name, "edges": edges }))
    }
}

fn with_neo4j_batch_backend(mut value: Value) -> Value {
    value["backend"] = json!({
        "mode": "neo4j",
        "atomicity": "Neo4j graph writes are committed in one transaction after all supported operations succeed; session-local schema metadata is staged and installed after commit."
    });
    value
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

fn parse_named_id_filter(raw: &BTreeMap<String, String>, key: &str) -> grm_rs::Result<Option<i64>> {
    raw.get(key)
        .map(|value| {
            value.parse::<i64>().map_err(|_| {
                GrmError::Constraint(format!("{key} filter must be an integer backend id"))
            })
        })
        .transpose()
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

#[tool_handler]
impl ServerHandler for GrmMcpServer {
    fn get_info(&self) -> ServerInfo {
        let instructions = if self.is_service() {
            "Use GRM tools against the configured gRPC workspace service. On startup call grm_schema_list, then inspect grm://backend/status. gRPC MCP mode supports schema define/list, grm_batch for schema/node/edge create/update/delete, node_create, node_update, node_delete, edge_create, edge_update, edge_delete, traversal-capable node_find for node or edge results, edge_find, grm_explain, and grm_profile through ExecuteWorkspace. Direct service RPC families, import/export, and free-form query parity are not supported yet."
        } else if self.is_neo4j() {
            "Use GRM tools to inspect session-local runtime schema and write supported schema-aware operations directly to Neo4j. On startup call grm_schema_list, then inspect grm://backend/status and grm://graph/summary; if schema_template_loaded is true, verify the recovered models before writing. If schema_template_persistence_enabled is true and schema_template_loaded is false, this server started with fresh local schema memory. If runtime schema is empty, ask whether to define or reconstruct schema before grm_batch writes. Neo4j mode supports schema define/list/checkpoint, grm_batch for schema/node/edge create/update/delete, node_create, node_update, node_delete, edge_create, edge_update, edge_delete, simple node/edge find, and graph summary counts for the current session-local runtime schema."
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
                serde_json::to_string_pretty(&self.summary_json().await.map_err(to_mcp_error)?)
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
    use grm_rs::{BackendIdType, PredicateOp, RuntimeField, RuntimeValueType};

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
    fn neo4j_batch_response_preserves_backend_metadata() {
        let value = with_neo4j_batch_backend(json!({
            "applied": true,
            "atomic": true,
            "operation_count": 0,
            "counts": {},
            "errors": [],
        }));

        assert_eq!(value["backend"]["mode"], json!("neo4j"));
        assert!(
            value["backend"]["atomicity"]
                .as_str()
                .unwrap()
                .contains("one transaction")
        );
    }
}
