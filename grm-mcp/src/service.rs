use std::collections::BTreeMap;
use std::sync::Arc;

use grm_rs::{
    EdgeFindRequest, GrmError, NodeFindRequest, OrderDirection, PredicateOp, QueryTerm,
    Result as GrmResult, SessionBatchEndpoint, SessionBatchOp, SessionBatchParams,
    SessionBatchResponse, TraversalDirection, TraversalReturn,
};
use grm_service_api::{GrpcClientTlsOptions, grpc_channel, proto};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;
use tonic::transport::Channel;

use crate::schema::{
    DefineEdgeParams, DefineNodeParams, EdgeCreateParams, EdgeDeleteParams, EdgeFindParams,
    EdgeUpdateParams, FieldParam, NodeCreateParams, NodeDeleteParams, NodeFindParams,
    NodeUpdateParams,
};

#[derive(Clone)]
pub(crate) struct ServiceMcpBackend {
    endpoint: String,
    workspace: proto::WorkspaceRef,
    handle: proto::WorkspaceHandle,
    format: ServiceWorkspaceFormat,
    tls_enabled: bool,
    client_certificate_configured: bool,
    client: Arc<Mutex<proto::grm_service_client::GrmServiceClient<Channel>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceWorkspaceMode {
    Create,
    Open,
}

impl ServiceWorkspaceMode {
    pub(crate) fn parse(raw: &str) -> GrmResult<Self> {
        match raw {
            "create" => Ok(Self::Create),
            "open" => Ok(Self::Open),
            other => Err(GrmError::Constraint(format!(
                "unsupported GRM_SERVICE_WORKSPACE_MODE '{other}'; expected 'create' or 'open'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceWorkspaceFormat {
    Json,
    Binary,
}

impl ServiceWorkspaceFormat {
    pub(crate) fn parse(raw: &str) -> GrmResult<Self> {
        match raw {
            "json" => Ok(Self::Json),
            "bin" | "binary" => Ok(Self::Binary),
            other => Err(GrmError::Constraint(format!(
                "unsupported GRM_SERVICE_WORKSPACE_FORMAT '{other}'; expected 'json', 'bin', or 'binary'"
            ))),
        }
    }

    pub(crate) fn as_proto_code(self) -> i32 {
        match self {
            Self::Json => proto::DurabilityFormat::Json as i32,
            Self::Binary => proto::DurabilityFormat::Binary as i32,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Binary => "binary",
        }
    }
}

impl ServiceMcpBackend {
    pub(crate) async fn connect(
        endpoint: String,
        workspace_id: String,
        mode: ServiceWorkspaceMode,
        format: ServiceWorkspaceFormat,
        tls: Option<GrpcClientTlsOptions>,
    ) -> GrmResult<Self> {
        let tls_enabled = tls.is_some();
        let client_certificate_configured =
            tls.as_ref().is_some_and(GrpcClientTlsOptions::has_identity);
        let workspace = proto::WorkspaceRef { id: workspace_id };
        let channel = grpc_channel(endpoint.clone(), tls.as_ref())
            .await
            .map_err(service_client_error)?;
        let mut client = proto::grm_service_client::GrmServiceClient::new(channel);
        let format_code = format.as_proto_code();
        let handle = match mode {
            ServiceWorkspaceMode::Create => client
                .create_workspace(proto::WorkspaceCreateRequest {
                    mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
                    workspace: Some(workspace.clone()),
                    format: format_code,
                })
                .await
                .map_err(service_status_error)?
                .into_inner()
                .handle
                .ok_or_else(|| missing_service_field("WorkspaceCreateResponse.handle"))?,
            ServiceWorkspaceMode::Open => client
                .open_workspace(proto::WorkspaceOpenRequest {
                    snapshot: None,
                    format: format_code,
                    workspace: Some(workspace.clone()),
                })
                .await
                .map_err(service_status_error)?
                .into_inner()
                .handle
                .ok_or_else(|| missing_service_field("WorkspaceOpenResponse.handle"))?,
        };
        Ok(Self {
            endpoint,
            workspace,
            handle,
            format,
            tls_enabled,
            client_certificate_configured,
            client: Arc::new(Mutex::new(client)),
        })
    }

    pub(crate) fn status_value(&self) -> Value {
        json!({
            "backend": {
                "mode": "grpc",
                "connected": true,
                "endpoint": self.endpoint,
                "workspace_ref": self.workspace.id,
                "workspace_handle": self.handle.id,
                "workspace_format": self.format.as_str(),
                "transport": if self.client_certificate_configured {
                    "tls-with-client-certificate"
                } else if self.tls_enabled {
                    "tls"
                } else {
                    "insecure-local-grpc"
                },
                "workspace_scope": "ExecuteWorkspace",
                "note": "MCP is using the GRM gRPC workspace service as the persisted operational-memory layer for the proven schema/CRUD/node.find traversal/find/batch subset.",
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
                    "grm_edge_find",
                    "grm_explain",
                    "grm_profile"
                ],
                "unsupported_surfaces": [
                    "snapshots",
                    "import/export",
                    "direct service RPC families",
                    "free-form query parity"
                ]
            },
            "recommended_startup_flow": [
                "Start the gRPC workspace service with a configured local workspace root.",
                "Start grm-mcp with GRM_BACKEND=grpc, GRM_SERVICE_ENDPOINT, GRM_WORKSPACE_REF, and GRM_SERVICE_WORKSPACE_MODE=create or open. GRM_SERVICE_WORKSPACE_FORMAT defaults to binary; set it to json only when you need explicit JSON workspace files. Set GRM_SERVICE_TLS_CA_CERT and GRM_SERVICE_TLS_DOMAIN_NAME to trust a local TLS service. Set GRM_SERVICE_TLS_CLIENT_CERT and GRM_SERVICE_TLS_CLIENT_KEY when the service requires mutual TLS.",
                "Call grm_schema_list to verify the workspace schema before writing.",
                "Use grm_batch or the schema/node/edge CRUD tools; MCP sends these through ExecuteWorkspace. grm_node_find also accepts via, end_filters, edge_filters, return, order, limit, and offset for traversal-shaped node or edge results. grm_explain and grm_profile support typed node.find and edge.find commands through ExecuteWorkspace."
            ]
        })
    }

    pub(crate) async fn schema_json(&self) -> GrmResult<Value> {
        let response = self
            .execute(proto::runtime_request::Request::SchemaList(
                proto::SchemaListRequest {},
            ))
            .await?;
        let Some(proto::runtime_response::Response::SchemaList(schema)) =
            response.response.and_then(|runtime| runtime.response)
        else {
            return Err(GrmError::Backend(
                "gRPC service returned unexpected schema list response".into(),
            ));
        };
        Ok(schema_list_value(
            schema,
            Some(self.status_value()["backend"].clone()),
        ))
    }

    pub(crate) async fn define_node(&self, params: DefineNodeParams) -> GrmResult<Value> {
        let _ = self
            .execute(proto::runtime_request::Request::DefineNode(
                proto::DefineNodeRequest {
                    name: params.name,
                    id_field: params.id_field,
                    fields: field_params(params.fields)?,
                },
            ))
            .await?;
        Ok(json!({ "applied": true }))
    }

    pub(crate) async fn define_edge(&self, params: DefineEdgeParams) -> GrmResult<Value> {
        let _ = self
            .execute(proto::runtime_request::Request::DefineEdge(
                proto::DefineEdgeRequest {
                    name: params.name,
                    from_model: params.from_model,
                    to_model: params.to_model,
                    id_field: params.id_field,
                    fields: field_params(params.fields)?,
                },
            ))
            .await?;
        Ok(json!({ "applied": true }))
    }

    pub(crate) async fn node_create(&self, params: NodeCreateParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::CreateNode(
            proto::NodeCreateRequest {
                model: params.model,
                props: Some(property_map(params.props)?),
            },
        ))
        .await
    }

    pub(crate) async fn node_update(&self, params: NodeUpdateParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::UpdateNode(
            proto::NodeUpdateRequest {
                model: params.model,
                id: params.id,
                props: Some(property_map(params.props)?),
            },
        ))
        .await
    }

    pub(crate) async fn node_delete(&self, params: NodeDeleteParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::DeleteNode(
            proto::NodeDeleteRequest {
                model: params.model,
                id: params.id,
            },
        ))
        .await
    }

    pub(crate) async fn node_find(&self, params: NodeFindParams) -> GrmResult<Value> {
        let request = params.into_node_find_request()?;
        self.execute_value(proto::runtime_request::Request::FindNodes(proto_node_find(
            request,
        )?))
        .await
    }

    pub(crate) async fn edge_create(&self, params: EdgeCreateParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::CreateEdge(
            proto::EdgeCreateRequest {
                model: params.model,
                from: params.from,
                to: params.to,
                props: Some(property_map(params.props)?),
            },
        ))
        .await
    }

    pub(crate) async fn edge_update(&self, params: EdgeUpdateParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::UpdateEdge(
            proto::EdgeUpdateRequest {
                model: params.model,
                id: params.id,
                props: Some(property_map(params.props)?),
            },
        ))
        .await
    }

    pub(crate) async fn edge_delete(&self, params: EdgeDeleteParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::DeleteEdge(
            proto::EdgeDeleteRequest {
                model: params.model,
                id: params.id,
            },
        ))
        .await
    }

    pub(crate) async fn edge_find(&self, params: EdgeFindParams) -> GrmResult<Value> {
        let request = EdgeFindRequest::from_adapter_filter_values(params.model, params.filters)?;
        self.execute_value(proto::runtime_request::Request::FindEdges(proto_edge_find(
            request,
        )?))
        .await
    }

    pub(crate) async fn batch(&self, params: SessionBatchParams) -> GrmResult<Value> {
        self.execute_value(proto::runtime_request::Request::ApplyBatch(proto_batch(
            params,
        )?))
        .await
    }

    pub(crate) async fn explain_node_find_terms(
        &self,
        model_name: &str,
        terms: &[QueryTerm],
    ) -> GrmResult<Value> {
        reject_introspection_format_terms(terms)?;
        let request = NodeFindRequest::from_adapter_query_terms(model_name, terms.iter().cloned())?;
        let response = self
            .execute_value(proto::runtime_request::Request::Explain(
                proto::ExplainRequest {
                    query: Some(proto::QueryRequest {
                        query: Some(proto::query_request::Query::NodeFind(
                            proto_node_find_shape(request)?,
                        )),
                    }),
                },
            ))
            .await?;
        Ok(explain_response_value("node.find", model_name, response))
    }

    pub(crate) async fn profile_node_find_terms(
        &self,
        model_name: &str,
        terms: &[QueryTerm],
    ) -> GrmResult<Value> {
        reject_introspection_format_terms(terms)?;
        let request = NodeFindRequest::from_adapter_query_terms(model_name, terms.iter().cloned())?;
        let response = self
            .execute_value(proto::runtime_request::Request::Profile(
                proto::ProfileRequest {
                    query: Some(proto::QueryRequest {
                        query: Some(proto::query_request::Query::NodeFind(
                            proto_node_find_shape(request)?,
                        )),
                    }),
                },
            ))
            .await?;
        profile_response_value("node.find", model_name, response)
    }

    pub(crate) async fn explain_edge_find_terms(
        &self,
        model_name: &str,
        terms: &[QueryTerm],
    ) -> GrmResult<Value> {
        reject_introspection_format_terms(terms)?;
        let request = EdgeFindRequest::from_adapter_filter_values(
            model_name,
            collect_query_term_values(terms),
        )?;
        let response = self
            .execute_value(proto::runtime_request::Request::Explain(
                proto::ExplainRequest {
                    query: Some(proto::QueryRequest {
                        query: Some(proto::query_request::Query::EdgeFind(
                            proto_edge_find_shape(request)?,
                        )),
                    }),
                },
            ))
            .await?;
        Ok(explain_response_value("edge.find", model_name, response))
    }

    pub(crate) async fn profile_edge_find_terms(
        &self,
        model_name: &str,
        terms: &[QueryTerm],
    ) -> GrmResult<Value> {
        reject_introspection_format_terms(terms)?;
        let request = EdgeFindRequest::from_adapter_filter_values(
            model_name,
            collect_query_term_values(terms),
        )?;
        let response = self
            .execute_value(proto::runtime_request::Request::Profile(
                proto::ProfileRequest {
                    query: Some(proto::QueryRequest {
                        query: Some(proto::query_request::Query::EdgeFind(
                            proto_edge_find_shape(request)?,
                        )),
                    }),
                },
            ))
            .await?;
        profile_response_value("edge.find", model_name, response)
    }

    async fn execute_value(&self, request: proto::runtime_request::Request) -> GrmResult<Value> {
        let response = self.execute(request).await?;
        let runtime = response
            .response
            .and_then(|response| response.response)
            .ok_or_else(|| {
                GrmError::Backend("gRPC service returned empty runtime response".into())
            })?;
        runtime_response_value(runtime)
    }

    async fn execute(
        &self,
        request: proto::runtime_request::Request,
    ) -> GrmResult<proto::WorkspaceRuntimeResponse> {
        let mut client = self.client.lock().await;
        client
            .execute_workspace(proto::WorkspaceRuntimeRequest {
                handle: Some(self.handle.clone()),
                request: Some(proto::RuntimeRequest {
                    request: Some(request),
                }),
            })
            .await
            .map_err(service_status_error)
            .map(|response| response.into_inner())
    }
}

fn runtime_response_value(response: proto::runtime_response::Response) -> GrmResult<Value> {
    use proto::runtime_response::Response;
    match response {
        Response::DefineNode(_) | Response::DefineEdge(_) | Response::SchemaList(_) => Err(
            GrmError::Backend("schema responses should be handled through schema_json".into()),
        ),
        Response::CreateNode(response) => stored_node_value(required(response.node, "node")?),
        Response::UpdateNode(response) => stored_node_value(required(response.node, "node")?),
        Response::DeleteNode(response) => {
            let deleted = required(response.deleted, "deleted")?;
            Ok(json!({ "deleted": true, "model": deleted.model, "id": deleted.id }))
        }
        Response::FindNodes(response) => {
            let edges = response
                .edges
                .into_iter()
                .map(stored_edge_value)
                .collect::<GrmResult<Vec<_>>>()?;
            let mut value = json!({
                "model": response.model,
                "nodes": response.nodes.into_iter().map(stored_node_value).collect::<GrmResult<Vec<_>>>()?,
            });
            if !edges.is_empty() {
                value["edges"] = json!(edges);
            }
            Ok(value)
        }
        Response::CreateEdge(response) => stored_edge_value(required(response.edge, "edge")?),
        Response::UpdateEdge(response) => stored_edge_value(required(response.edge, "edge")?),
        Response::DeleteEdge(response) => {
            let deleted = required(response.deleted, "deleted")?;
            Ok(json!({ "deleted": true, "model": deleted.model, "id": deleted.id }))
        }
        Response::FindEdges(response) => Ok(json!({
            "model": response.model,
            "edges": response.edges.into_iter().map(stored_edge_value).collect::<GrmResult<Vec<_>>>()?,
        })),
        Response::ApplyBatch(response) => batch_response_value(response),
        Response::Explain(response) => Ok(explain_proto_value(response)),
        Response::Profile(response) => Ok(profile_proto_value(response)?),
        Response::Query(_) => Err(GrmError::NotSupported(
            "gRPC MCP mode does not support free-form query parity yet",
        )),
        Response::IndexList(_) | Response::Summary(_) => Err(GrmError::NotSupported(
            "gRPC MCP mode does not support index/summary responses yet",
        )),
    }
}

fn batch_response_value(response: proto::BatchResponse) -> GrmResult<Value> {
    let mut counts = Map::new();
    for count in response.counts {
        let entry = counts.entry(count.op).or_insert_with(|| json!({}));
        let Value::Object(map) = entry else {
            return Err(GrmError::Backend("invalid batch count accumulator".into()));
        };
        map.insert(count.model, json!(count.count));
    }
    Ok(json!({
        "applied": response.applied,
        "atomic": response.atomic,
        "operation_count": response.operation_count,
        "counts": counts,
        "errors": response.errors.into_iter().map(|error| {
            json!({
                "index": error.index,
                "message": error.message,
                "recovery_hint": error.recovery_hint,
            })
        }).collect::<Vec<_>>(),
        "ids": response.ids.into_iter().map(|id| {
            json!({
                "op": id.op,
                "model": id.model,
                "id": id.id,
                "ref": id.local_ref,
            })
        }).collect::<Vec<_>>(),
    }))
}

fn explain_response_value(command: &str, target: &str, response: Value) -> Value {
    json!({
        "command": command,
        "target": target,
        "plan": response["plan"].clone(),
    })
}

fn profile_response_value(command: &str, target: &str, response: Value) -> GrmResult<Value> {
    Ok(json!({
        "command": command,
        "target": target,
        "plan": response["plan"].clone(),
        "result_rows": response["result_rows"].clone(),
        "elapsed": response["elapsed"].clone(),
        "per_step_metrics": Value::Null,
    }))
}

fn explain_proto_value(response: proto::ExplainResponse) -> Value {
    json!({
        "plan": {
            "kind": response.plan_kind,
            "steps": response.steps,
            "text": response.steps.join("\n"),
            "indexes": response.indexes,
        }
    })
}

fn profile_proto_value(response: proto::ProfileResponse) -> GrmResult<Value> {
    let plan = response
        .plan
        .ok_or_else(|| missing_service_field("ProfileResponse.plan"))?;
    Ok(json!({
        "plan": explain_proto_value(plan)["plan"].clone(),
        "result_rows": response.row_count,
        "elapsed": {
            "micros": response.elapsed_micros,
            "display": format!("{}us", response.elapsed_micros),
        }
    }))
}

fn schema_list_value(response: proto::SchemaListResponse, backend: Option<Value>) -> Value {
    let mut value = json!({
        "identity": {
            "node": id_type_keyword(response.backend_id_type),
            "edge": id_type_keyword(response.backend_id_type),
        },
        "nodes": response.node_models.into_iter().map(node_model_value).collect::<Vec<_>>(),
        "edges": response.edge_models.into_iter().map(edge_model_value).collect::<Vec<_>>(),
    });
    if let Some(backend) = backend {
        value["backend"] = backend;
    }
    value
}

fn node_model_value(model: proto::NodeModel) -> Value {
    json!({
        "name": model.name,
        "label": model.label,
        "id_field_name": model.id_field_name,
        "id_type": id_type_display(model.id_type),
        "fields": model.fields.into_iter().map(field_value).collect::<Vec<_>>(),
    })
}

fn edge_model_value(model: proto::EdgeModel) -> Value {
    json!({
        "name": model.name,
        "rel_type": model.rel_type,
        "from_model": model.from_model,
        "to_model": model.to_model,
        "id_field_name": model.id_field_name,
        "id_type": id_type_display(model.id_type),
        "fields": model.fields.into_iter().map(field_value).collect::<Vec<_>>(),
    })
}

fn field_value(field: proto::FieldSpec) -> Value {
    json!({
        "name": field.name,
        "value_type": field_type_display(field.value_type),
        "required": field.required,
    })
}

fn stored_node_value(node: proto::StoredNode) -> GrmResult<Value> {
    Ok(json!({
        "id": node.id,
        "labels": node.labels,
        "props": property_map_value(node.props)?,
    }))
}

fn stored_edge_value(edge: proto::StoredEdge) -> GrmResult<Value> {
    Ok(json!({
        "id": edge.id,
        "rel_type": edge.rel_type,
        "from": edge.from,
        "to": edge.to,
        "props": property_map_value(edge.props)?,
    }))
}

fn property_map_value(map: Option<proto::PropertyMap>) -> GrmResult<Value> {
    let mut object = Map::new();
    if let Some(map) = map {
        for property in map.properties {
            object.insert(
                property.name,
                property_value_to_json(required(property.value, "property.value")?)?,
            );
        }
    }
    Ok(Value::Object(object))
}

fn property_value_to_json(value: proto::PropertyValue) -> GrmResult<Value> {
    use proto::property_value::Kind;
    match required(value.kind, "property.kind")? {
        Kind::StringValue(value) => Ok(json!(value)),
        Kind::IntValue(value) => Ok(json!(value)),
        Kind::FloatValue(value) => Ok(json!(value)),
        Kind::BoolValue(value) => Ok(json!(value)),
    }
}

fn proto_batch(params: SessionBatchParams) -> GrmResult<proto::BatchRequest> {
    Ok(proto::BatchRequest {
        atomic: params.atomic,
        allow_deletes: params.allow_deletes,
        response_mode: match params.response {
            SessionBatchResponse::Summary => proto::BatchResponseMode::Summary as i32,
            SessionBatchResponse::Detailed => proto::BatchResponseMode::Detailed as i32,
        },
        ops: params
            .ops
            .into_iter()
            .map(proto_batch_op)
            .collect::<GrmResult<Vec<_>>>()?,
    })
}

fn proto_batch_op(op: SessionBatchOp) -> GrmResult<proto::BatchOperation> {
    use proto::batch_operation::Op;
    let op = match op {
        SessionBatchOp::SchemaDefineNode(params) => {
            Op::SchemaDefineNode(proto::DefineNodeRequest {
                name: params.name,
                id_field: params.id_field,
                fields: batch_fields(params.fields)?,
            })
        }
        SessionBatchOp::SchemaDefineEdge(params) => {
            Op::SchemaDefineEdge(proto::DefineEdgeRequest {
                name: params.name,
                from_model: params.from_model,
                to_model: params.to_model,
                id_field: params.id_field,
                fields: batch_fields(params.fields)?,
            })
        }
        SessionBatchOp::NodeCreate(params) => Op::NodeCreate(proto::BatchNodeCreate {
            model: params.model,
            props: Some(property_map(params.props)?),
            local_ref: params.local_ref,
        }),
        SessionBatchOp::NodeUpdate(params) => Op::NodeUpdate(proto::NodeUpdateRequest {
            model: params.model,
            id: params.id,
            props: Some(property_map(params.props)?),
        }),
        SessionBatchOp::NodeDelete(params) => Op::NodeDelete(proto::NodeDeleteRequest {
            model: params.model,
            id: params.id,
        }),
        SessionBatchOp::EdgeCreate(params) => Op::EdgeCreate(proto::BatchEdgeCreate {
            model: params.model,
            from: Some(proto_batch_endpoint(params.from)),
            to: Some(proto_batch_endpoint(params.to)),
            props: Some(property_map(params.props)?),
        }),
        SessionBatchOp::EdgeUpdate(params) => Op::EdgeUpdate(proto::EdgeUpdateRequest {
            model: params.model,
            id: params.id,
            props: Some(property_map(params.props)?),
        }),
        SessionBatchOp::EdgeDelete(params) => Op::EdgeDelete(proto::EdgeDeleteRequest {
            model: params.model,
            id: params.id,
        }),
    };
    Ok(proto::BatchOperation { op: Some(op) })
}

fn proto_batch_endpoint(endpoint: SessionBatchEndpoint) -> proto::BatchEndpoint {
    use proto::batch_endpoint::Endpoint;
    proto::BatchEndpoint {
        endpoint: Some(match endpoint {
            SessionBatchEndpoint::Id(id) => Endpoint::Id(id),
            SessionBatchEndpoint::Ref(local_ref) => Endpoint::LocalRef(local_ref),
        }),
    }
}

fn proto_node_find(request: NodeFindRequest) -> GrmResult<proto::NodeFindRequest> {
    let shape = proto_node_find_shape(request)?;
    Ok(proto::NodeFindRequest {
        model: shape.model,
        predicates: shape.predicates,
        end_predicates: shape.end_predicates,
        edge_predicates: shape.edge_predicates,
        traversals: shape.traversals,
        order: shape.order,
        limit: shape.limit,
        offset: shape.offset,
        id: shape.id,
        return_mode: shape.return_mode,
    })
}

fn proto_node_find_shape(request: NodeFindRequest) -> GrmResult<proto::NodeFindShape> {
    Ok(proto::NodeFindShape {
        model: request.model,
        predicates: request
            .predicates
            .into_iter()
            .map(proto_predicate)
            .collect::<GrmResult<Vec<_>>>()?,
        end_predicates: request
            .end_predicates
            .into_iter()
            .map(proto_predicate)
            .collect::<GrmResult<Vec<_>>>()?,
        edge_predicates: request
            .edge_predicates
            .into_iter()
            .map(proto_predicate)
            .collect::<GrmResult<Vec<_>>>()?,
        traversals: request
            .traversals
            .into_iter()
            .map(|step| proto::TraversalStep {
                direction: proto_traversal_direction(step.direction),
                edge_model: step.edge_model,
                end_model: step.end_model,
            })
            .collect(),
        order: request.order.into_iter().map(proto_order).collect(),
        limit: request.limit.map(usize_to_u64).transpose()?,
        offset: request.offset.map(usize_to_u64).transpose()?,
        id: request.id,
        return_mode: request.return_mode.map(proto_traversal_return),
    })
}

fn proto_edge_find(request: EdgeFindRequest) -> GrmResult<proto::EdgeFindRequest> {
    let shape = proto_edge_find_shape(request)?;
    Ok(proto::EdgeFindRequest {
        model: shape.model,
        predicates: shape.predicates,
        order: shape.order,
        limit: shape.limit,
        offset: shape.offset,
        id: shape.id,
        from: shape.from,
        to: shape.to,
    })
}

fn proto_edge_find_shape(request: EdgeFindRequest) -> GrmResult<proto::EdgeFindShape> {
    Ok(proto::EdgeFindShape {
        model: request.model,
        predicates: request
            .predicates
            .into_iter()
            .map(proto_predicate)
            .collect::<GrmResult<Vec<_>>>()?,
        order: request.order.into_iter().map(proto_order).collect(),
        limit: request.limit.map(usize_to_u64).transpose()?,
        offset: request.offset.map(usize_to_u64).transpose()?,
        id: request.id,
        from: request.from,
        to: request.to,
    })
}

fn collect_query_term_values(terms: &[QueryTerm]) -> BTreeMap<String, Value> {
    terms
        .iter()
        .map(|term| (term.key.clone(), json!(term.value)))
        .collect()
}

fn reject_introspection_format_terms(terms: &[QueryTerm]) -> GrmResult<()> {
    if terms.iter().any(|term| term.key == "format") {
        return Err(GrmError::NotSupported(
            "format= is not supported with session.explain or session.profile",
        ));
    }
    Ok(())
}

fn proto_predicate(predicate: grm_rs::PropertyPredicate) -> GrmResult<proto::PropertyPredicate> {
    Ok(proto::PropertyPredicate {
        field: predicate.field,
        op: proto_predicate_op(predicate.op),
        value: Some(property_value(predicate.value)?),
    })
}

fn proto_order(order: grm_rs::OrderSpec) -> proto::OrderSpec {
    proto::OrderSpec {
        field: order.field,
        direction: match order.direction {
            OrderDirection::Asc => proto::OrderDirection::Asc as i32,
            OrderDirection::Desc => proto::OrderDirection::Desc as i32,
        },
    }
}

fn property_map(values: BTreeMap<String, Value>) -> GrmResult<proto::PropertyMap> {
    Ok(proto::PropertyMap {
        properties: values
            .into_iter()
            .map(|(name, value)| {
                Ok(proto::Property {
                    name,
                    value: Some(property_value(value)?),
                })
            })
            .collect::<GrmResult<Vec<_>>>()?,
    })
}

fn property_value(value: Value) -> GrmResult<proto::PropertyValue> {
    use proto::property_value::Kind;
    let kind = match value {
        Value::String(value) => Kind::StringValue(value),
        Value::Bool(value) => Kind::BoolValue(value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Kind::IntValue(value)
            } else if let Some(value) = value.as_u64() {
                let value = i64::try_from(value).map_err(|_| {
                    GrmError::Constraint("integer property value is too large for int64".into())
                })?;
                Kind::IntValue(value)
            } else if let Some(value) = value.as_f64() {
                Kind::FloatValue(value)
            } else {
                return Err(GrmError::Constraint(
                    "numeric property value cannot be represented in service proto".into(),
                ));
            }
        }
        Value::Null => {
            return Err(GrmError::Constraint(
                "null is not a supported graph value; omit the field instead".into(),
            ));
        }
        Value::Array(_) | Value::Object(_) => {
            return Err(GrmError::Constraint(
                "graph values must be strings, numbers, or booleans".into(),
            ));
        }
    };
    Ok(proto::PropertyValue { kind: Some(kind) })
}

fn field_params(fields: Vec<FieldParam>) -> GrmResult<Vec<proto::FieldSpec>> {
    fields
        .into_iter()
        .map(|field| {
            Ok(proto::FieldSpec {
                name: field.name,
                value_type: proto_field_type(&field.value_type)?,
                required: field.required,
            })
        })
        .collect()
}

fn batch_fields(fields: Vec<grm_rs::SessionBatchFieldParam>) -> GrmResult<Vec<proto::FieldSpec>> {
    fields
        .into_iter()
        .map(|field| {
            Ok(proto::FieldSpec {
                name: field.name,
                value_type: proto_field_type(&field.value_type)?,
                required: field.required,
            })
        })
        .collect()
}

fn proto_field_type(value_type: &str) -> GrmResult<i32> {
    match value_type {
        "string" => Ok(proto::FieldValueType::String as i32),
        "int" => Ok(proto::FieldValueType::Int as i32),
        "float" => Ok(proto::FieldValueType::Float as i32),
        "bool" => Ok(proto::FieldValueType::Bool as i32),
        other => Err(GrmError::Constraint(format!(
            "unsupported field type '{other}'; expected string, int, float, or bool"
        ))),
    }
}

fn proto_predicate_op(op: PredicateOp) -> i32 {
    match op {
        PredicateOp::Eq => proto::PredicateOp::Eq as i32,
        PredicateOp::Ne => proto::PredicateOp::Ne as i32,
        PredicateOp::Gt => proto::PredicateOp::Gt as i32,
        PredicateOp::Ge => proto::PredicateOp::Ge as i32,
        PredicateOp::Lt => proto::PredicateOp::Lt as i32,
        PredicateOp::Le => proto::PredicateOp::Le as i32,
        PredicateOp::Contains => proto::PredicateOp::Contains as i32,
    }
}

fn proto_traversal_direction(direction: TraversalDirection) -> i32 {
    match direction {
        TraversalDirection::Out => proto::TraversalDirection::Out as i32,
        TraversalDirection::In => proto::TraversalDirection::In as i32,
        TraversalDirection::Both => proto::TraversalDirection::Both as i32,
    }
}

fn proto_traversal_return(return_mode: TraversalReturn) -> i32 {
    match return_mode {
        TraversalReturn::End => proto::TraversalReturn::End as i32,
        TraversalReturn::Root => proto::TraversalReturn::Root as i32,
        TraversalReturn::Edge => proto::TraversalReturn::Edge as i32,
    }
}

fn id_type_keyword(value: i32) -> &'static str {
    match proto::IdType::try_from(value).ok() {
        Some(proto::IdType::Int64) => "int",
        _ => "unknown",
    }
}

fn id_type_display(value: i32) -> &'static str {
    match proto::IdType::try_from(value).ok() {
        Some(proto::IdType::Int64) => "Int64",
        _ => "Unspecified",
    }
}

fn field_type_display(value: i32) -> &'static str {
    match proto::FieldValueType::try_from(value).ok() {
        Some(proto::FieldValueType::String) => "String",
        Some(proto::FieldValueType::Int) => "Int",
        Some(proto::FieldValueType::Float) => "Float",
        Some(proto::FieldValueType::Bool) => "Bool",
        _ => "Unspecified",
    }
}

fn usize_to_u64(value: usize) -> GrmResult<u64> {
    u64::try_from(value).map_err(|_| GrmError::Constraint("value is too large for u64".into()))
}

fn required<T>(value: Option<T>, field: &'static str) -> GrmResult<T> {
    value.ok_or_else(|| missing_service_field(field))
}

fn missing_service_field(field: &'static str) -> GrmError {
    GrmError::Backend(format!(
        "gRPC service response missing required field '{field}'"
    ))
}

fn service_status_error(status: tonic::Status) -> GrmError {
    GrmError::Backend(format!("gRPC service error: {status}"))
}

fn service_client_error(error: grm_service_api::GrpcWorkspaceClientError) -> GrmError {
    GrmError::Backend(format!("gRPC service connection failed: {error}"))
}
