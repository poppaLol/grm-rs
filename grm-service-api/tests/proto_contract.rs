use std::fs;

use grm_rs::{
    BatchRequest, DurableOperation, EdgeRequest, NodeRequest, NodeResponse, QueryRequest,
    RuntimeDelete, RuntimeDispatchOutcome, RuntimeRequest, RuntimeResponse, SchemaRequest,
    SchemaResponse, SessionState,
};
use grm_service_api as svc;
use grm_service_api::proto::{DefineNodeRequest, FieldSpec, FieldValueType};
use grm_service_api::{PROTO_FILES, ServiceRequest, proto, proto_files, proto_root};
use serde_json::json;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

#[test]
fn proto_files_are_packaged() {
    let files = proto_files().collect::<Vec<_>>();

    assert_eq!(files.len(), PROTO_FILES.len());
    for file in files {
        assert!(file.exists(), "missing proto file {}", file.display());
    }
}

#[test]
fn service_surface_covers_runtime_request_families() {
    let service = read_proto("grm/service/v1/service.proto");

    for rpc in [
        "CreateWorkspace",
        "OpenWorkspace",
        "ExecuteWorkspace",
        "CloseWorkspace",
        "DefineNode",
        "DefineEdge",
        "SchemaList",
        "CreateNode",
        "UpdateNode",
        "DeleteNode",
        "FindNodes",
        "CreateEdge",
        "UpdateEdge",
        "DeleteEdge",
        "FindEdges",
        "Query",
        "Explain",
        "Profile",
        "ApplyBatch",
        "Save",
        "Load",
        "Export",
        "Import",
        "IndexList",
        "Summary",
    ] {
        assert!(
            service.contains(&format!("rpc {rpc}(")),
            "missing rpc {rpc}"
        );
    }
}

#[test]
fn proto_contract_compiles_with_codegen() {
    let out_dir = tempfile::tempdir().expect("temporary output directory");
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc path");
    let files = proto_files().collect::<Vec<_>>();
    let includes = [proto_root()];

    let mut config = prost_build::Config::new();
    config.out_dir(out_dir.path());
    config.protoc_executable(protoc);
    config
        .compile_protos(&files, &includes)
        .expect("proto contract should compile");
}

#[test]
fn proto_contract_keeps_query_typed_instead_of_textual() {
    let joined = all_proto_text();

    assert!(
        !joined.contains("command_text"),
        "service contract must not expose CLI command text"
    );
    assert!(
        !joined.contains("string query ="),
        "query contract must be typed request messages, not a query string"
    );
    assert!(
        joined.contains("message QueryRequest") && joined.contains("oneof query"),
        "typed query union is missing"
    );
}

#[test]
fn public_admin_contract_does_not_accept_server_file_paths() {
    let proto = all_proto_text();

    assert!(
        !proto.contains("string path") && !proto.contains("optional string path"),
        "public service proto must not expose client-supplied server file paths"
    );
    assert!(
        proto.contains("message SnapshotHandle")
            && proto.contains("bytes document")
            && proto.contains("message WorkspaceHandle")
            && proto.contains("message WorkspaceRuntimeRequest"),
        "service proto should use managed handles and bytes for workspace/snapshot flows"
    );
}

#[test]
fn write_responses_expose_durable_mutation_outcomes() {
    for (file, message) in [
        ("grm/service/v1/schema.proto", "DefineNodeResponse"),
        ("grm/service/v1/schema.proto", "DefineEdgeResponse"),
        ("grm/service/v1/node.proto", "NodeCreateResponse"),
        ("grm/service/v1/node.proto", "NodeUpdateResponse"),
        ("grm/service/v1/node.proto", "NodeDeleteResponse"),
        ("grm/service/v1/edge.proto", "EdgeCreateResponse"),
        ("grm/service/v1/edge.proto", "EdgeUpdateResponse"),
        ("grm/service/v1/edge.proto", "EdgeDeleteResponse"),
        ("grm/service/v1/batch.proto", "BatchResponse"),
        ("grm/service/v1/admin.proto", "ImportResponse"),
    ] {
        let proto = read_proto(file);
        let body = message_body(&proto, message);
        assert!(
            body.contains("DurableMutationOutcome durability"),
            "{message} must include durable mutation outcome"
        );
    }
}

#[test]
fn durable_operation_shape_matches_current_runtime_outcome() {
    let proto = read_proto("grm/service/v1/common.proto");
    for variant in [
        "register_node_model",
        "register_edge_model",
        "upsert_node",
        "delete_node_id",
        "upsert_edge",
        "delete_edge_id",
        "batch",
    ] {
        assert!(
            proto.contains(variant),
            "DurableOperation proto missing {variant}"
        );
    }

    let outcome = RuntimeDispatchOutcome {
        response: RuntimeResponse::Node(NodeResponse::Delete(RuntimeDelete {
            model: "User".into(),
            id: 7,
        })),
        durable_ops: vec![DurableOperation::DeleteNode { id: 7 }],
    };

    assert_eq!(outcome.durable_ops.len(), 1);
    assert!(matches!(
        outcome.durable_ops.as_slice(),
        [DurableOperation::DeleteNode { id: 7 }]
    ));
}

#[test]
fn runtime_family_mapping_notes_stay_true_for_public_types() {
    let requests = [
        RuntimeRequest::Schema(SchemaRequest::DefineNode(grm_rs::DefineNodeRequest {
            name: "User".into(),
            id_field: "user_id".into(),
            fields: Vec::new(),
        })),
        RuntimeRequest::Node(NodeRequest::Create(grm_rs::NodeCreateRequest {
            model: "User".into(),
            props: [("name".into(), json!("Ada"))].into_iter().collect(),
        })),
        RuntimeRequest::Edge(EdgeRequest::Find(grm_rs::EdgeFindRequest {
            model: "Follows".into(),
            ..Default::default()
        })),
        RuntimeRequest::Query(QueryRequest::NodeFind(grm_rs::NodeFindRequest {
            model: "User".into(),
            ..Default::default()
        })),
        RuntimeRequest::Batch(BatchRequest {
            atomic: true,
            allow_deletes: false,
            response: grm_rs::SessionBatchResponse::Summary,
            ops: Vec::new(),
        }),
    ];

    let mapped = requests
        .into_iter()
        .map(|request| match request {
            RuntimeRequest::Schema(_) => "schema",
            RuntimeRequest::Node(_) => "node",
            RuntimeRequest::Edge(_) => "edge",
            RuntimeRequest::Query(_) => "query",
            RuntimeRequest::Explain(_) => "explain",
            RuntimeRequest::Profile(_) => "profile",
            RuntimeRequest::Batch(_) => "batch",
            RuntimeRequest::Admin(_) => "admin",
        })
        .collect::<Vec<_>>();

    assert_eq!(mapped, ["schema", "node", "edge", "query", "batch"]);
}

#[tokio::test]
async fn generated_proto_schema_request_executes_through_runtime_dispatcher() {
    let mut state = SessionState::new();

    let generated = DefineNodeRequest {
        name: "User".into(),
        id_field: "userId".into(),
        fields: vec![FieldSpec {
            name: "name".into(),
            value_type: FieldValueType::String as i32,
            required: true,
        }],
    };
    let request = ServiceRequest::DefineNode(generated.try_into().unwrap());

    let outcome = request.execute(&mut state).await.unwrap();

    assert!(matches!(
        outcome.response,
        RuntimeResponse::Schema(SchemaResponse::DefineNode(model))
            if model.name == "User" && model.fields[0].name == "name"
    ));
    assert!(matches!(
        outcome.durable_ops.as_slice(),
        [DurableOperation::RegisterNodeModel { model }] if model.name == "User"
    ));
}

#[tokio::test]
async fn generated_proto_batch_request_executes_existing_runtime_batch_path() {
    let mut state = SessionState::new();

    let generated = proto::BatchRequest {
        atomic: true,
        allow_deletes: false,
        response_mode: proto::BatchResponseMode::Detailed as i32,
        ops: vec![
            proto::BatchOperation {
                op: Some(proto::batch_operation::Op::SchemaDefineNode(
                    proto::DefineNodeRequest {
                        name: "User".into(),
                        id_field: "userId".into(),
                        fields: vec![proto::FieldSpec {
                            name: "name".into(),
                            value_type: proto::FieldValueType::String as i32,
                            required: true,
                        }],
                    },
                )),
            },
            proto::BatchOperation {
                op: Some(proto::batch_operation::Op::NodeCreate(
                    proto::BatchNodeCreate {
                        model: "User".into(),
                        props: Some(proto_property_map([(
                            "name",
                            proto::property_value::Kind::StringValue("Ada".into()),
                        )])),
                        local_ref: Some("ada".into()),
                    },
                )),
            },
        ],
    };
    let request = svc::ServiceRequest::ApplyBatch(generated.try_into().unwrap());

    let outcome = request.execute(&mut state).await.unwrap();

    assert!(matches!(
        outcome.response,
        RuntimeResponse::Batch(batch)
            if batch.should_persist
                && batch.value["applied"] == json!(true)
                && batch.value["ids"][0]["ref"] == json!("ada")
    ));
    assert!(matches!(
        outcome.durable_ops.as_slice(),
        [DurableOperation::Batch { ops }] if ops.len() == 2
    ));
}

#[tokio::test]
async fn service_shaped_node_and_edge_requests_execute_through_runtime_dispatcher() {
    let mut state = SessionState::new();
    define_user_post_schema(&mut state).await;

    let user_outcome = svc::ServiceRequest::CreateNode(svc::NodeCreateRequest {
        model: "User".into(),
        props: property_map([("name", svc::PropertyValue::String("Ada".into()))]),
    })
    .execute(&mut state)
    .await
    .unwrap();
    let RuntimeResponse::Node(grm_rs::NodeResponse::Create(user)) = user_outcome.response else {
        panic!("expected node create response");
    };
    assert!(matches!(
        user_outcome.durable_ops.as_slice(),
        [DurableOperation::UpsertNode { node }] if node.id == user.id
    ));

    let post_outcome = svc::ServiceRequest::CreateNode(svc::NodeCreateRequest {
        model: "Post".into(),
        props: property_map([]),
    })
    .execute(&mut state)
    .await
    .unwrap();
    let RuntimeResponse::Node(grm_rs::NodeResponse::Create(post)) = post_outcome.response else {
        panic!("expected node create response");
    };

    let edge_outcome = svc::ServiceRequest::CreateEdge(svc::EdgeCreateRequest {
        model: "Authored".into(),
        from: user.id,
        to: post.id,
        props: property_map([("year", svc::PropertyValue::Int(2026))]),
    })
    .execute(&mut state)
    .await
    .unwrap();

    assert!(matches!(
        edge_outcome.response,
        RuntimeResponse::Edge(grm_rs::EdgeResponse::Create(edge))
            if edge.from == user.id && edge.to == post.id
    ));
    assert!(matches!(
        edge_outcome.durable_ops.as_slice(),
        [DurableOperation::UpsertRel { rel }] if rel.rel_type == "Authored"
    ));

    let found = svc::ServiceRequest::FindEdges(svc::EdgeFindRequest {
        model: "Authored".into(),
        predicates: Vec::new(),
        order: Vec::new(),
        limit: None,
        offset: None,
        id: None,
        from: Some(user.id),
        to: Some(post.id),
    })
    .execute(&mut state)
    .await
    .unwrap();
    assert!(matches!(
        found.response,
        RuntimeResponse::Edge(grm_rs::EdgeResponse::Find(found))
            if found.model == "Authored" && found.edges.len() == 1
    ));
    assert!(found.durable_ops.is_empty());
}

#[tokio::test]
async fn service_shaped_unsupported_request_returns_explicit_runtime_error() {
    let mut state = SessionState::new();

    let err = svc::ServiceRequest::Query(svc::QueryRequest {
        query: svc::Query::Traversal(svc::TraversalRequest {
            root: svc::NodeFindShape {
                model: "User".into(),
                predicates: Vec::new(),
                end_predicates: Vec::new(),
                edge_predicates: Vec::new(),
                traversals: Vec::new(),
                order: Vec::new(),
                limit: None,
                offset: None,
                id: None,
                return_mode: None,
            },
        }),
    })
    .execute(&mut state)
    .await
    .unwrap_err();

    assert!(matches!(err, grm_rs::GrmError::NotSupported(_)));
    assert!(err.to_string().contains("traversal query requests yet"));
}

#[tokio::test]
async fn in_process_workspace_service_executes_generated_requests_against_handle() {
    let mut service = svc::InProcessWorkspaceService::new();
    let created = service
        .create_workspace(
            proto::WorkspaceCreateRequest {
                mode: proto::WorkspaceCreateMode::InMemory as i32,
                workspace: None,
                format: proto::DurabilityFormat::Json as i32,
            }
            .try_into()
            .unwrap(),
        )
        .unwrap();
    let generated_created: proto::WorkspaceCreateResponse = created.clone().into();
    assert_eq!(generated_created.handle.unwrap().id, created.handle.id);
    assert!(!created.handle.id.is_empty());

    let generated_schema = proto::DefineNodeRequest {
        name: "User".into(),
        id_field: "userId".into(),
        fields: vec![proto::FieldSpec {
            name: "name".into(),
            value_type: proto::FieldValueType::String as i32,
            required: true,
        }],
    };
    let schema_response = service
        .execute_runtime(
            proto::WorkspaceRuntimeRequest {
                handle: Some(created.handle.clone().into()),
                request: Some(proto::RuntimeRequest {
                    request: Some(proto::runtime_request::Request::DefineNode(
                        generated_schema,
                    )),
                }),
            }
            .try_into()
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(schema_response.handle, created.handle);
    assert!(matches!(
        schema_response.response,
        RuntimeResponse::Schema(grm_rs::SchemaResponse::DefineNode(ref model))
            if model.name == "User"
    ));
    assert!(matches!(
        schema_response.durable_operations.as_slice(),
        [DurableOperation::RegisterNodeModel { model }] if model.name == "User"
    ));
    let generated_schema_response: proto::WorkspaceRuntimeResponse =
        schema_response.try_into().unwrap();
    assert!(matches!(
        generated_schema_response
            .response
            .and_then(|response| response.response),
        Some(proto::runtime_response::Response::DefineNode(response))
            if response.model.as_ref().unwrap().name == "User"
                && response.durability.as_ref().unwrap().has_durable_mutation
    ));

    let generated_batch = proto::BatchRequest {
        atomic: true,
        allow_deletes: false,
        response_mode: proto::BatchResponseMode::Detailed as i32,
        ops: vec![proto::BatchOperation {
            op: Some(proto::batch_operation::Op::NodeCreate(
                proto::BatchNodeCreate {
                    model: "User".into(),
                    props: Some(proto_property_map([(
                        "name",
                        proto::property_value::Kind::StringValue("Ada".into()),
                    )])),
                    local_ref: Some("ada".into()),
                },
            )),
        }],
    };
    let batch_response = service
        .execute_runtime(
            proto::WorkspaceRuntimeRequest {
                handle: Some(created.handle.clone().into()),
                request: Some(proto::RuntimeRequest {
                    request: Some(proto::runtime_request::Request::ApplyBatch(generated_batch)),
                }),
            }
            .try_into()
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(batch_response.handle, created.handle);
    assert!(matches!(
        batch_response.response,
        RuntimeResponse::Batch(ref batch)
            if batch.value["applied"] == json!(true)
                && batch.value["ids"][0]["ref"] == json!("ada")
    ));
    assert!(matches!(
        batch_response.durable_operations.as_slice(),
        [DurableOperation::UpsertNode { node }] if node.labels.iter().any(|label| label == "User")
    ));
    let generated_batch_response: proto::WorkspaceRuntimeResponse =
        batch_response.try_into().unwrap();
    assert!(matches!(
        generated_batch_response
            .response
            .and_then(|response| response.response),
        Some(proto::runtime_response::Response::ApplyBatch(response))
            if response.applied
                && response.ids[0].local_ref.as_deref() == Some("ada")
                && response.durability.as_ref().unwrap().durable_op_count == 1
    ));
}

#[tokio::test]
async fn in_process_workspace_service_reopens_closed_loop_snapshot_by_handle() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace.json");
    let mut service = svc::InProcessWorkspaceService::new();
    let created = service
        .create_workspace(svc::WorkspaceCreateRequest {
            mode: svc::WorkspaceCreateMode::InMemory,
            workspace: None,
            format: svc::DurabilityFormat::Json,
        })
        .unwrap();

    service
        .execute_runtime(svc::WorkspaceRuntimeRequest {
            handle: created.handle.clone(),
            request: svc::ServiceRequest::DefineNode(svc::DefineNodeRequest {
                name: "User".into(),
                id_field: "userId".into(),
                fields: vec![svc::FieldSpec {
                    name: "email".into(),
                    value_type: svc::FieldValueType::String,
                    required: false,
                }],
            }),
        })
        .await
        .unwrap();
    service
        .execute_runtime(svc::WorkspaceRuntimeRequest {
            handle: created.handle.clone(),
            request: create_user_request("Ada"),
        })
        .await
        .unwrap();
    let snapshot = service
        .save_workspace_to_local_snapshot(
            &created.handle,
            svc::LocalWorkspaceSnapshotRequest {
                format: svc::DurabilityFormat::Json,
                path: path.clone(),
            },
        )
        .unwrap();
    let closed = service
        .close_workspace(
            proto::WorkspaceCloseRequest {
                handle: Some(created.handle.clone().into()),
            }
            .try_into()
            .unwrap(),
        )
        .unwrap();
    let generated_closed: proto::WorkspaceCloseResponse = closed.into();
    assert_eq!(generated_closed.handle.unwrap().id, created.handle.id);

    let opened = service
        .open_workspace(
            proto::WorkspaceOpenRequest {
                snapshot: Some(snapshot.into()),
                workspace: None,
                format: proto::DurabilityFormat::Json as i32,
            }
            .try_into()
            .unwrap(),
        )
        .unwrap();
    let generated_opened: proto::WorkspaceOpenResponse = opened.clone().into();
    assert_eq!(generated_opened.handle.unwrap().id, opened.handle.id);
    assert_ne!(opened.handle, created.handle);

    let reopened = service.workspace(&opened.handle).unwrap();
    let model = reopened.state().model("User").unwrap();
    assert_eq!(model.origin, grm_rs::RuntimeSchemaOrigin::Declared);
    assert!(model.field("email").is_some());
    assert_workspace_users(reopened, ["Ada"]).await;

    service
        .execute_runtime(svc::WorkspaceRuntimeRequest {
            handle: opened.handle.clone(),
            request: create_user_request("Grace"),
        })
        .await
        .unwrap();
    let snapshot = service
        .save_workspace_to_local_snapshot(
            &opened.handle,
            svc::LocalWorkspaceSnapshotRequest {
                format: svc::DurabilityFormat::Json,
                path,
            },
        )
        .unwrap();
    service
        .close_workspace(svc::WorkspaceCloseRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();

    let reopened_again = service
        .open_workspace(svc::WorkspaceOpenRequest {
            snapshot: Some(snapshot),
            workspace: None,
            format: svc::DurabilityFormat::Json,
        })
        .unwrap();
    assert_ne!(reopened_again.handle, opened.handle);

    let reopened_again = service.workspace(&reopened_again.handle).unwrap();
    let model = reopened_again.state().model("User").unwrap();
    assert_eq!(model.origin, grm_rs::RuntimeSchemaOrigin::Declared);
    assert!(model.field("email").is_some());
    assert_workspace_users(reopened_again, ["Ada", "Grace"]).await;
}

#[tokio::test]
async fn in_process_workspace_service_returns_structured_errors() {
    let mut service = svc::InProcessWorkspaceService::new();
    let unknown = svc::WorkspaceHandle {
        id: "missing-workspace".into(),
    };

    let err = service
        .execute_runtime(svc::WorkspaceRuntimeRequest {
            handle: unknown.clone(),
            request: svc::ServiceRequest::SchemaList(svc::SchemaListRequest {}),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        svc::WorkspaceServiceError::UnknownWorkspaceHandle { handle }
            if handle == unknown
    ));

    let err = service
        .unsupported_workspace_operation(svc::WorkspaceUnsupportedRequest {
            operation: svc::WorkspaceUnsupportedOperation::OpenLoopExternalInference,
        })
        .unwrap_err();
    assert!(matches!(
        err,
        svc::WorkspaceServiceError::UnsupportedWorkspaceOperation("open-loop external inference")
    ));
}

#[tokio::test]
async fn generated_grpc_client_executes_workspace_requests_over_local_transport() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service =
        svc::GrpcWorkspaceService::new(svc::ServiceSecurityConfig::anonymous_local()).into_server();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let mut client = proto::grm_service_client::GrmServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap();

    let created = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::InMemory as i32,
            workspace: None,
            format: proto::DurabilityFormat::Json as i32,
        })
        .await
        .unwrap()
        .into_inner();
    let handle = created.handle.clone().unwrap();
    assert!(!handle.id.is_empty());

    let schema = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::DefineNode(
                    proto::DefineNodeRequest {
                        name: "User".into(),
                        id_field: "userId".into(),
                        fields: vec![proto::FieldSpec {
                            name: "name".into(),
                            value_type: proto::FieldValueType::String as i32,
                            required: true,
                        }],
                    },
                )),
            }),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        schema.response.and_then(|response| response.response),
        Some(proto::runtime_response::Response::DefineNode(response))
            if response.model.as_ref().unwrap().name == "User"
    ));

    let batch = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::ApplyBatch(
                    proto::BatchRequest {
                        atomic: true,
                        allow_deletes: false,
                        response_mode: proto::BatchResponseMode::Detailed as i32,
                        ops: vec![proto::BatchOperation {
                            op: Some(proto::batch_operation::Op::NodeCreate(
                                proto::BatchNodeCreate {
                                    model: "User".into(),
                                    props: Some(proto_property_map([(
                                        "name",
                                        proto::property_value::Kind::StringValue("Ada".into()),
                                    )])),
                                    local_ref: Some("ada".into()),
                                },
                            )),
                        }],
                    },
                )),
            }),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        batch.response.and_then(|response| response.response),
        Some(proto::runtime_response::Response::ApplyBatch(response))
            if response.applied && response.ids[0].local_ref.as_deref() == Some("ada")
    ));

    let missing = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(proto::WorkspaceHandle {
                id: "missing-workspace".into(),
            }),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::SchemaList(
                    proto::SchemaListRequest {},
                )),
            }),
        })
        .await
        .unwrap_err();
    assert_eq!(missing.code(), tonic::Code::NotFound);
    assert!(missing.message().contains("unknown workspace handle"));

    let unsupported = client
        .schema_list(proto::SchemaListRequest {})
        .await
        .unwrap_err();
    assert_eq!(unsupported.code(), tonic::Code::Unimplemented);
    assert!(unsupported.message().contains("ExecuteWorkspace"));

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn workspace_client_executes_through_workspace_scope() {
    let temp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = svc::GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        svc::ServiceSecurityConfig::anonymous_local(),
    )
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

    let workspace_ref = "workspace-client-smoke";
    let mut client = svc::GrpcWorkspaceClient::connect(
        format!("http://{addr}"),
        workspace_ref,
        svc::GrpcWorkspaceMode::Create,
    )
    .await
    .unwrap();
    assert_eq!(client.workspace_ref().id, workspace_ref);

    let defined = client
        .execute_proto(proto::runtime_request::Request::DefineNode(
            proto::DefineNodeRequest {
                name: "ClientUser".into(),
                id_field: "userId".into(),
                fields: vec![proto::FieldSpec {
                    name: "name".into(),
                    value_type: proto::FieldValueType::String as i32,
                    required: true,
                }],
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        defined.response.and_then(|response| response.response),
        Some(proto::runtime_response::Response::DefineNode(response))
            if response.model.as_ref().unwrap().name == "ClientUser"
    ));

    let created = client
        .execute_proto(proto::runtime_request::Request::CreateNode(
            proto::NodeCreateRequest {
                model: "ClientUser".into(),
                props: Some(proto_property_map([(
                    "name",
                    proto::property_value::Kind::StringValue("Ada".into()),
                )])),
            },
        ))
        .await
        .unwrap();
    let node_id = created_node_id(created);
    assert!(temp.path().join(format!("{workspace_ref}.bin")).exists());
    assert!(!temp.path().join(format!("{workspace_ref}.json")).exists());
    client.close().await.unwrap();

    let mut reopened = svc::GrpcWorkspaceClient::connect(
        format!("http://{addr}"),
        workspace_ref,
        svc::GrpcWorkspaceMode::Open,
    )
    .await
    .unwrap();
    let found = reopened
        .execute_proto(proto::runtime_request::Request::FindNodes(
            find_nodes_by_id_proto("ClientUser", node_id),
        ))
        .await
        .unwrap();
    assert_eq!(node_string_props(found, "name"), vec!["Ada"]);

    reopened.close().await.unwrap();
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn generated_grpc_create_rejects_existing_workspace_without_replacing_it() {
    let temp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = svc::GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        svc::ServiceSecurityConfig::anonymous_local(),
    )
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

    let workspace = proto::WorkspaceRef {
        id: "existing-workspace".into(),
    };
    let mut client = proto::grm_service_client::GrmServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap();
    let created = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
            workspace: Some(workspace.clone()),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap()
        .into_inner();
    let handle = created.handle.unwrap();

    execute_workspace_proto(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(define_node_proto(
            "User",
            "userId",
            [("name", proto::FieldValueType::String, true)],
        )),
    )
    .await;
    execute_workspace_proto(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateNode(node_create_proto(
            "User",
            [(
                "name",
                proto::property_value::Kind::StringValue("Ada".into()),
            )],
        )),
    )
    .await;
    client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await
        .unwrap();

    let error = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
            workspace: Some(workspace.clone()),
            format: proto::DurabilityFormat::Json as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::AlreadyExists);
    assert_eq!(
        error.message(),
        "workspace 'existing-workspace' already exists; use open mode"
    );
    assert!(!error.message().contains(&temp.path().display().to_string()));
    assert!(temp.path().join("existing-workspace.bin").exists());
    assert!(!temp.path().join("existing-workspace.json").exists());

    let opened = client
        .open_workspace(proto::WorkspaceOpenRequest {
            workspace: Some(workspace),
            snapshot: None,
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap()
        .into_inner();
    let found = execute_workspace_proto(
        &mut client,
        &opened.handle.unwrap(),
        proto::runtime_request::Request::FindNodes(find_all_nodes_proto("User")),
    )
    .await;
    assert_eq!(node_string_props(found, "name"), vec!["Ada"]);

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn ergonomic_workspace_client_routes_supported_operations_through_execute_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = svc::GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        svc::ServiceSecurityConfig::anonymous_local(),
    )
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

    let mut client = svc::GrpcWorkspaceClient::connect(
        format!("http://{addr}"),
        "ergonomic-client-smoke",
        svc::GrpcWorkspaceMode::Create,
    )
    .await
    .unwrap();
    client
        .define_node(grm_rs::DefineNodeRequest {
            name: "User".into(),
            id_field: "userId".into(),
            fields: vec![grm_rs::FieldSpec {
                name: "name".into(),
                value_type: grm_rs::FieldValueType::String,
                required: true,
            }],
        })
        .await
        .unwrap();
    client
        .define_node(grm_rs::DefineNodeRequest {
            name: "Post".into(),
            id_field: "postId".into(),
            fields: vec![grm_rs::FieldSpec {
                name: "title".into(),
                value_type: grm_rs::FieldValueType::String,
                required: true,
            }],
        })
        .await
        .unwrap();
    client
        .define_edge(grm_rs::DefineEdgeRequest {
            name: "Authored".into(),
            from_model: "User".into(),
            to_model: "Post".into(),
            id_field: "authoredId".into(),
            fields: vec![grm_rs::FieldSpec {
                name: "year".into(),
                value_type: grm_rs::FieldValueType::Int,
                required: true,
            }],
        })
        .await
        .unwrap();
    let created = client
        .create_node(grm_rs::NodeCreateRequest {
            model: "User".into(),
            props: [("name".into(), json!("Ada"))].into_iter().collect(),
        })
        .await
        .unwrap();
    let post = client
        .create_node(grm_rs::NodeCreateRequest {
            model: "Post".into(),
            props: [("title".into(), json!("Traversal"))].into_iter().collect(),
        })
        .await
        .unwrap();
    client
        .create_edge(grm_rs::EdgeCreateRequest {
            model: "Authored".into(),
            from: created.id,
            to: post.id,
            props: [("year".into(), json!(2026))].into_iter().collect(),
        })
        .await
        .unwrap();
    let found = client
        .find_nodes(grm_rs::NodeFindRequest {
            model: "User".into(),
            id: Some(created.id),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(found.nodes.len(), 1);
    assert_eq!(found.nodes[0].props["name"], json!("Ada"));

    let traversed = client
        .find_nodes(grm_rs::NodeFindRequest {
            model: "User".into(),
            predicates: vec![grm_rs::PropertyPredicate {
                field: "name".into(),
                op: grm_rs::PredicateOp::Eq,
                value: json!("Ada"),
            }],
            traversals: vec![grm_rs::TraversalStepRequest {
                direction: grm_rs::TraversalDirection::Out,
                edge_model: Some("Authored".into()),
                end_model: "Post".into(),
            }],
            end_predicates: vec![grm_rs::PropertyPredicate {
                field: "title".into(),
                op: grm_rs::PredicateOp::Eq,
                value: json!("Traversal"),
            }],
            edge_predicates: vec![grm_rs::PropertyPredicate {
                field: "year".into(),
                op: grm_rs::PredicateOp::Eq,
                value: json!(2026),
            }],
            order: vec![grm_rs::OrderSpec {
                field: "title".into(),
                direction: grm_rs::OrderDirection::Asc,
            }],
            limit: Some(1),
            offset: Some(0),
            return_mode: Some(grm_rs::TraversalReturn::End),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(traversed.nodes.len(), 1);
    assert_eq!(traversed.nodes[0].id, post.id);
    assert_eq!(traversed.nodes[0].props["title"], json!("Traversal"));

    let traversal_request = grm_rs::NodeFindRequest {
        model: "User".into(),
        predicates: vec![grm_rs::PropertyPredicate {
            field: "name".into(),
            op: grm_rs::PredicateOp::Eq,
            value: json!("Ada"),
        }],
        traversals: vec![grm_rs::TraversalStepRequest {
            direction: grm_rs::TraversalDirection::Out,
            edge_model: Some("Authored".into()),
            end_model: "Post".into(),
        }],
        end_predicates: vec![grm_rs::PropertyPredicate {
            field: "title".into(),
            op: grm_rs::PredicateOp::Eq,
            value: json!("Traversal"),
        }],
        return_mode: Some(grm_rs::TraversalReturn::End),
        ..Default::default()
    };
    let explain = client
        .explain(grm_rs::ExplainRequest {
            query: grm_rs::QueryRequest::NodeFind(traversal_request.clone()),
        })
        .await
        .unwrap();
    assert_eq!(explain.plan_kind, "node.find");
    assert!(explain.steps.iter().any(|step| step.contains("ExpandOut")));
    assert!(
        explain
            .indexes
            .iter()
            .any(|index| index == "system.edge.outgoing_adjacency")
    );

    let profile = client
        .profile(grm_rs::ProfileRequest {
            query: grm_rs::QueryRequest::NodeFind(traversal_request),
        })
        .await
        .unwrap();
    assert_eq!(profile.plan.as_ref().unwrap().plan_kind, "node.find");
    assert_eq!(profile.row_count, 1);
    assert!(profile.elapsed_micros > 0);

    let edge_explain = client
        .explain(grm_rs::ExplainRequest {
            query: grm_rs::QueryRequest::EdgeFind(grm_rs::EdgeFindRequest {
                model: "Authored".into(),
                from: Some(created.id),
                ..Default::default()
            }),
        })
        .await
        .unwrap();
    assert_eq!(edge_explain.plan_kind, "edge.find");
    assert!(
        edge_explain
            .steps
            .iter()
            .any(|step| step.contains("RelationshipEndpointSeek"))
    );

    let edge_return = client
        .find_node_results(grm_rs::NodeFindRequest {
            model: "User".into(),
            traversals: vec![grm_rs::TraversalStepRequest {
                direction: grm_rs::TraversalDirection::Out,
                edge_model: Some("Authored".into()),
                end_model: "Post".into(),
            }],
            return_mode: Some(grm_rs::TraversalReturn::Edge),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(edge_return.nodes.is_empty());
    assert_eq!(edge_return.edges.len(), 1);
    assert_eq!(edge_return.edges[0].rel_type, "Authored");
    assert_eq!(edge_return.edges[0].from, created.id);
    assert_eq!(edge_return.edges[0].to, post.id);
    assert_eq!(edge_return.edges[0].props["year"], json!(2026));

    let node_only_edge_return = client
        .find_nodes(grm_rs::NodeFindRequest {
            model: "User".into(),
            traversals: vec![grm_rs::TraversalStepRequest {
                direction: grm_rs::TraversalDirection::Out,
                edge_model: Some("Authored".into()),
                end_model: "Post".into(),
            }],
            return_mode: Some(grm_rs::TraversalReturn::Edge),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(node_only_edge_return.to_string().contains("find_nodes"));
    assert!(temp.path().join("ergonomic-client-smoke.bin").exists());

    client.close().await.unwrap();
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn workspace_client_accepts_explicit_json_workspace_format() {
    let temp = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = svc::GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        svc::ServiceSecurityConfig::anonymous_local(),
    )
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

    let workspace_ref = "workspace-client-json-smoke";
    let client = svc::GrpcWorkspaceClient::connect_with_format(
        format!("http://{addr}"),
        workspace_ref,
        svc::GrpcWorkspaceMode::Create,
        svc::DurabilityFormat::Json,
    )
    .await
    .unwrap();
    let mut client = client;
    client
        .execute_proto(proto::runtime_request::Request::DefineNode(
            proto::DefineNodeRequest {
                name: "JsonClientUser".into(),
                id_field: "userId".into(),
                fields: Vec::new(),
            },
        ))
        .await
        .unwrap();
    client.close().await.unwrap();

    assert!(temp.path().join(format!("{workspace_ref}.json")).exists());
    assert!(!temp.path().join(format!("{workspace_ref}.bin")).exists());

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn generated_grpc_client_reopens_binary_autocommitted_workspace_without_manual_save() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = proto::WorkspaceRef {
        id: "grpc_parity_workspace".into(),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = svc::GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        svc::ServiceSecurityConfig::anonymous_local(),
    )
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

    let mut client = proto::grm_service_client::GrmServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap();

    let direct = client
        .define_node(proto::DefineNodeRequest {
            name: "DirectOnly".into(),
            id_field: "directOnlyId".into(),
            fields: Vec::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(direct.code(), tonic::Code::Unimplemented);
    assert!(direct.message().contains("ExecuteWorkspace"));

    let direct = client
        .create_node(proto::NodeCreateRequest {
            model: "DirectOnly".into(),
            props: None,
        })
        .await
        .unwrap_err();
    assert_eq!(direct.code(), tonic::Code::Unimplemented);
    assert!(direct.message().contains("ExecuteWorkspace"));

    let direct = client
        .apply_batch(proto::BatchRequest {
            atomic: true,
            allow_deletes: false,
            response_mode: proto::BatchResponseMode::Detailed as i32,
            ops: Vec::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(direct.code(), tonic::Code::Unimplemented);
    assert!(direct.message().contains("ExecuteWorkspace"));

    let created = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
            workspace: Some(workspace.clone()),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(created.workspace.as_ref(), Some(&workspace));
    let opened_handle = created.handle.unwrap();
    assert!(!opened_handle.id.is_empty());
    assert!(temp.path().join("grpc_parity_workspace.bin").exists());
    assert!(!temp.path().join("grpc_parity_workspace.json").exists());

    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::DefineNode(define_node_proto(
            "User",
            "userId",
            [("name", proto::FieldValueType::String, true)],
        )),
    )
    .await;
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::DefineNode(define_node_proto(
            "Post",
            "postId",
            [("title", proto::FieldValueType::String, true)],
        )),
    )
    .await;
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::DefineEdge(define_edge_proto(
            "Authored",
            "User",
            "Post",
            "authoredId",
            [("year", proto::FieldValueType::Int, false)],
        )),
    )
    .await;

    let ada = created_node_id(
        execute_workspace_proto(
            &mut client,
            &opened_handle,
            proto::runtime_request::Request::CreateNode(node_create_proto(
                "User",
                [(
                    "name",
                    proto::property_value::Kind::StringValue("Ada".into()),
                )],
            )),
        )
        .await,
    );
    let post = created_node_id(
        execute_workspace_proto(
            &mut client,
            &opened_handle,
            proto::runtime_request::Request::CreateNode(node_create_proto(
                "Post",
                [(
                    "title",
                    proto::property_value::Kind::StringValue("Parity notes".into()),
                )],
            )),
        )
        .await,
    );
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::UpdateNode(node_update_proto(
            "User",
            ada,
            [(
                "name",
                proto::property_value::Kind::StringValue("Ada Lovelace".into()),
            )],
        )),
    )
    .await;
    let found = execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::FindNodes(find_nodes_by_id_proto("User", ada)),
    )
    .await;
    assert_eq!(node_string_props(found, "name"), vec!["Ada Lovelace"]);

    let authored = created_edge_id(
        execute_workspace_proto(
            &mut client,
            &opened_handle,
            proto::runtime_request::Request::CreateEdge(edge_create_proto(
                "Authored",
                ada,
                post,
                [("year", proto::property_value::Kind::IntValue(2026))],
            )),
        )
        .await,
    );
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::UpdateEdge(edge_update_proto(
            "Authored",
            authored,
            [("year", proto::property_value::Kind::IntValue(2027))],
        )),
    )
    .await;
    let found = execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::FindEdges(find_edges_by_id_proto("Authored", authored)),
    )
    .await;
    assert_eq!(edge_int_props(found, "year"), vec![2027]);

    let traversal_shape = proto::NodeFindShape {
        model: "User".into(),
        predicates: vec![proto_predicate(
            "name",
            proto::property_value::Kind::StringValue("Ada Lovelace".into()),
        )],
        end_predicates: vec![proto_predicate(
            "title",
            proto::property_value::Kind::StringValue("Parity notes".into()),
        )],
        edge_predicates: vec![proto_predicate(
            "year",
            proto::property_value::Kind::IntValue(2027),
        )],
        traversals: vec![proto::TraversalStep {
            direction: proto::TraversalDirection::Out as i32,
            edge_model: Some("Authored".into()),
            end_model: "Post".into(),
        }],
        order: Vec::new(),
        limit: None,
        offset: None,
        id: None,
        return_mode: Some(proto::TraversalReturn::End as i32),
    };
    let explain = execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::Explain(proto::ExplainRequest {
            query: Some(proto::QueryRequest {
                query: Some(proto::query_request::Query::NodeFind(
                    traversal_shape.clone(),
                )),
            }),
        }),
    )
    .await;
    let Some(proto::runtime_response::Response::Explain(explain)) =
        explain.response.and_then(|response| response.response)
    else {
        panic!("expected workspace explain response");
    };
    assert_eq!(explain.plan_kind, "node.find");
    assert!(explain.steps.iter().any(|step| step.contains("ExpandOut")));

    let profile = execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::Profile(proto::ProfileRequest {
            query: Some(proto::QueryRequest {
                query: Some(proto::query_request::Query::NodeFind(traversal_shape)),
            }),
        }),
    )
    .await;
    let Some(proto::runtime_response::Response::Profile(profile)) =
        profile.response.and_then(|response| response.response)
    else {
        panic!("expected workspace profile response");
    };
    assert_eq!(profile.plan.as_ref().unwrap().plan_kind, "node.find");
    assert_eq!(profile.row_count, 1);

    let temporary_user = created_node_id(
        execute_workspace_proto(
            &mut client,
            &opened_handle,
            proto::runtime_request::Request::CreateNode(node_create_proto(
                "User",
                [(
                    "name",
                    proto::property_value::Kind::StringValue("Temporary User".into()),
                )],
            )),
        )
        .await,
    );
    let temporary_post = created_node_id(
        execute_workspace_proto(
            &mut client,
            &opened_handle,
            proto::runtime_request::Request::CreateNode(node_create_proto(
                "Post",
                [(
                    "title",
                    proto::property_value::Kind::StringValue("Temporary Post".into()),
                )],
            )),
        )
        .await,
    );
    let temporary_edge = created_edge_id(
        execute_workspace_proto(
            &mut client,
            &opened_handle,
            proto::runtime_request::Request::CreateEdge(edge_create_proto(
                "Authored",
                temporary_user,
                temporary_post,
                [("year", proto::property_value::Kind::IntValue(2026))],
            )),
        )
        .await,
    );
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::DeleteEdge(proto::EdgeDeleteRequest {
            model: "Authored".into(),
            id: temporary_edge,
        }),
    )
    .await;
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::DeleteNode(proto::NodeDeleteRequest {
            model: "User".into(),
            id: temporary_user,
        }),
    )
    .await;
    execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::DeleteNode(proto::NodeDeleteRequest {
            model: "Post".into(),
            id: temporary_post,
        }),
    )
    .await;
    let found = execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::FindEdges(find_edges_by_id_proto(
            "Authored",
            temporary_edge,
        )),
    )
    .await;
    assert!(edge_int_props(found, "year").is_empty());

    let batch = execute_workspace_proto(
        &mut client,
        &opened_handle,
        proto::runtime_request::Request::ApplyBatch(proto::BatchRequest {
            atomic: true,
            allow_deletes: false,
            response_mode: proto::BatchResponseMode::Detailed as i32,
            ops: vec![
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::SchemaDefineNode(
                        define_node_proto(
                            "Tag",
                            "tagId",
                            [("label", proto::FieldValueType::String, true)],
                        ),
                    )),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::SchemaDefineEdge(
                        define_edge_proto("Tagged", "User", "Tag", "taggedId", []),
                    )),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::NodeCreate(
                        proto::BatchNodeCreate {
                            model: "Tag".into(),
                            props: Some(proto_property_map([(
                                "label",
                                proto::property_value::Kind::StringValue("service".into()),
                            )])),
                            local_ref: Some("service-tag".into()),
                        },
                    )),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::EdgeCreate(
                        proto::BatchEdgeCreate {
                            model: "Tagged".into(),
                            from: Some(proto::BatchEndpoint {
                                endpoint: Some(proto::batch_endpoint::Endpoint::Id(ada)),
                            }),
                            to: Some(proto::BatchEndpoint {
                                endpoint: Some(proto::batch_endpoint::Endpoint::LocalRef(
                                    "service-tag".into(),
                                )),
                            }),
                            props: None,
                        },
                    )),
                },
            ],
        }),
    )
    .await;
    assert!(matches!(
        batch.response.and_then(|response| response.response),
        Some(proto::runtime_response::Response::ApplyBatch(response))
            if response.applied
                && response.ids.iter().any(|id| id.local_ref.as_deref() == Some("service-tag"))
                && response.durability.as_ref().unwrap().has_durable_mutation
    ));

    client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(opened_handle),
        })
        .await
        .unwrap();

    let reopened = client
        .open_workspace(proto::WorkspaceOpenRequest {
            snapshot: None,
            workspace: Some(workspace.clone()),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reopened.workspace.as_ref(), Some(&workspace));
    let reopened_handle = reopened.handle.unwrap();

    let grace = created_node_id(
        execute_workspace_proto(
            &mut client,
            &reopened_handle,
            proto::runtime_request::Request::CreateNode(node_create_proto(
                "User",
                [(
                    "name",
                    proto::property_value::Kind::StringValue("Grace Hopper".into()),
                )],
            )),
        )
        .await,
    );
    assert_ne!(grace, ada);

    let users = execute_workspace_proto(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::FindNodes(find_all_nodes_proto("User")),
    )
    .await;
    assert_eq!(
        sorted(node_string_props(users, "name")),
        vec!["Ada Lovelace", "Grace Hopper"]
    );
    let posts = execute_workspace_proto(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::FindNodes(find_all_nodes_proto("Post")),
    )
    .await;
    assert_eq!(node_string_props(posts, "title"), vec!["Parity notes"]);
    let edges = execute_workspace_proto(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::FindEdges(find_edges_by_id_proto("Authored", authored)),
    )
    .await;
    assert_eq!(edge_int_props(edges, "year"), vec![2027]);
    let tags = execute_workspace_proto(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::FindNodes(find_all_nodes_proto("Tag")),
    )
    .await;
    assert_eq!(node_string_props(tags, "label"), vec!["service"]);

    let schema = execute_workspace_proto(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
    )
    .await;
    let Some(proto::runtime_response::Response::SchemaList(schema)) =
        schema.response.and_then(|response| response.response)
    else {
        panic!("expected generated SchemaList response");
    };
    let node_models = schema
        .node_models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    assert!(node_models.contains(&"User"));
    assert!(node_models.contains(&"Post"));
    assert!(node_models.contains(&"Tag"));
    let edge_models = schema
        .edge_models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    assert!(edge_models.contains(&"Authored"));
    assert!(edge_models.contains(&"Tagged"));

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

fn read_proto(relative: &str) -> String {
    fs::read_to_string(proto_root().join(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

fn all_proto_text() -> String {
    proto_files()
        .map(|file| fs::read_to_string(file).expect("proto file should be readable"))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn execute_workspace_proto(
    client: &mut proto::grm_service_client::GrmServiceClient<tonic::transport::Channel>,
    handle: &proto::WorkspaceHandle,
    request: proto::runtime_request::Request,
) -> proto::WorkspaceRuntimeResponse {
    client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(request),
            }),
        })
        .await
        .unwrap()
        .into_inner()
}

fn define_node_proto<const N: usize>(
    name: &str,
    id_field: &str,
    fields: [(&str, proto::FieldValueType, bool); N],
) -> proto::DefineNodeRequest {
    proto::DefineNodeRequest {
        name: name.into(),
        id_field: id_field.into(),
        fields: fields
            .into_iter()
            .map(|(name, value_type, required)| proto::FieldSpec {
                name: name.into(),
                value_type: value_type as i32,
                required,
            })
            .collect(),
    }
}

fn define_edge_proto<const N: usize>(
    name: &str,
    from_model: &str,
    to_model: &str,
    id_field: &str,
    fields: [(&str, proto::FieldValueType, bool); N],
) -> proto::DefineEdgeRequest {
    proto::DefineEdgeRequest {
        name: name.into(),
        from_model: from_model.into(),
        to_model: to_model.into(),
        id_field: id_field.into(),
        fields: fields
            .into_iter()
            .map(|(name, value_type, required)| proto::FieldSpec {
                name: name.into(),
                value_type: value_type as i32,
                required,
            })
            .collect(),
    }
}

fn node_create_proto<const N: usize>(
    model: &str,
    props: [(&str, proto::property_value::Kind); N],
) -> proto::NodeCreateRequest {
    proto::NodeCreateRequest {
        model: model.into(),
        props: Some(proto_property_map(props)),
    }
}

fn node_update_proto<const N: usize>(
    model: &str,
    id: i64,
    props: [(&str, proto::property_value::Kind); N],
) -> proto::NodeUpdateRequest {
    proto::NodeUpdateRequest {
        model: model.into(),
        id,
        props: Some(proto_property_map(props)),
    }
}

fn edge_create_proto<const N: usize>(
    model: &str,
    from: i64,
    to: i64,
    props: [(&str, proto::property_value::Kind); N],
) -> proto::EdgeCreateRequest {
    proto::EdgeCreateRequest {
        model: model.into(),
        from,
        to,
        props: Some(proto_property_map(props)),
    }
}

fn edge_update_proto<const N: usize>(
    model: &str,
    id: i64,
    props: [(&str, proto::property_value::Kind); N],
) -> proto::EdgeUpdateRequest {
    proto::EdgeUpdateRequest {
        model: model.into(),
        id,
        props: Some(proto_property_map(props)),
    }
}

fn find_all_nodes_proto(model: &str) -> proto::NodeFindRequest {
    proto::NodeFindRequest {
        model: model.into(),
        predicates: Vec::new(),
        end_predicates: Vec::new(),
        edge_predicates: Vec::new(),
        traversals: Vec::new(),
        order: Vec::new(),
        limit: None,
        offset: None,
        id: None,
        return_mode: None,
    }
}

fn find_nodes_by_id_proto(model: &str, id: i64) -> proto::NodeFindRequest {
    proto::NodeFindRequest {
        id: Some(id),
        ..find_all_nodes_proto(model)
    }
}

fn find_edges_by_id_proto(model: &str, id: i64) -> proto::EdgeFindRequest {
    proto::EdgeFindRequest {
        model: model.into(),
        predicates: Vec::new(),
        order: Vec::new(),
        limit: None,
        offset: None,
        id: Some(id),
        from: None,
        to: None,
    }
}

fn proto_predicate(field: &str, value: proto::property_value::Kind) -> proto::PropertyPredicate {
    proto::PropertyPredicate {
        field: field.into(),
        op: proto::PredicateOp::Eq as i32,
        value: Some(proto::PropertyValue { kind: Some(value) }),
    }
}

fn created_node_id(response: proto::WorkspaceRuntimeResponse) -> i64 {
    match response.response.and_then(|response| response.response) {
        Some(proto::runtime_response::Response::CreateNode(response)) => response.node.unwrap().id,
        other => panic!("expected generated NodeCreate response, got {other:?}"),
    }
}

fn created_edge_id(response: proto::WorkspaceRuntimeResponse) -> i64 {
    match response.response.and_then(|response| response.response) {
        Some(proto::runtime_response::Response::CreateEdge(response)) => response.edge.unwrap().id,
        other => panic!("expected generated EdgeCreate response, got {other:?}"),
    }
}

fn node_string_props(response: proto::WorkspaceRuntimeResponse, field: &str) -> Vec<String> {
    let Some(proto::runtime_response::Response::FindNodes(response)) =
        response.response.and_then(|response| response.response)
    else {
        panic!("expected generated NodeFind response");
    };

    response
        .nodes
        .into_iter()
        .filter_map(|node| proto_string_prop(node.props, field))
        .collect()
}

fn edge_int_props(response: proto::WorkspaceRuntimeResponse, field: &str) -> Vec<i64> {
    let Some(proto::runtime_response::Response::FindEdges(response)) =
        response.response.and_then(|response| response.response)
    else {
        panic!("expected generated EdgeFind response");
    };

    response
        .edges
        .into_iter()
        .filter_map(|edge| proto_int_prop(edge.props, field))
        .collect()
}

fn proto_string_prop(props: Option<proto::PropertyMap>, field: &str) -> Option<String> {
    props?
        .properties
        .into_iter()
        .find(|property| property.name == field)
        .and_then(|property| property.value)
        .and_then(|value| match value.kind {
            Some(proto::property_value::Kind::StringValue(value)) => Some(value),
            _ => None,
        })
}

fn proto_int_prop(props: Option<proto::PropertyMap>, field: &str) -> Option<i64> {
    props?
        .properties
        .into_iter()
        .find(|property| property.name == field)
        .and_then(|property| property.value)
        .and_then(|value| match value.kind {
            Some(proto::property_value::Kind::IntValue(value)) => Some(value),
            _ => None,
        })
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

fn message_body<'a>(proto: &'a str, message: &str) -> &'a str {
    let marker = format!("message {message} {{");
    let start = proto
        .find(&marker)
        .unwrap_or_else(|| panic!("missing message {message}"))
        + marker.len();
    let rest = &proto[start..];
    let end = rest
        .find("\n}")
        .unwrap_or_else(|| panic!("missing end for message {message}"));
    &rest[..end]
}

async fn define_user_post_schema(state: &mut SessionState) {
    for request in [
        svc::ServiceRequest::DefineNode(svc::DefineNodeRequest {
            name: "User".into(),
            id_field: "userId".into(),
            fields: vec![svc::FieldSpec {
                name: "name".into(),
                value_type: svc::FieldValueType::String,
                required: true,
            }],
        }),
        svc::ServiceRequest::DefineNode(svc::DefineNodeRequest {
            name: "Post".into(),
            id_field: "postId".into(),
            fields: Vec::new(),
        }),
        svc::ServiceRequest::DefineEdge(svc::DefineEdgeRequest {
            name: "Authored".into(),
            from_model: "User".into(),
            to_model: "Post".into(),
            id_field: "authoredId".into(),
            fields: vec![svc::FieldSpec {
                name: "year".into(),
                value_type: svc::FieldValueType::Int,
                required: true,
            }],
        }),
    ] {
        request.execute(state).await.unwrap();
    }
}

fn create_user_request(name: &str) -> svc::ServiceRequest {
    svc::ServiceRequest::CreateNode(svc::NodeCreateRequest {
        model: "User".into(),
        props: property_map([(
            "email",
            svc::PropertyValue::String(format!("{name}@example.test")),
        )]),
    })
}

async fn assert_workspace_users<const N: usize>(
    workspace: &grm_rs::Workspace,
    expected: [&str; N],
) {
    let users = workspace
        .state()
        .node_find_response(grm_rs::NodeFindRequest {
            model: "User".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut emails = users
        .nodes
        .iter()
        .map(|node| node.props["email"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    emails.sort();

    let mut expected = expected
        .into_iter()
        .map(|name| format!("{name}@example.test"))
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(emails, expected);
}

fn property_map<const N: usize>(properties: [(&str, svc::PropertyValue); N]) -> svc::PropertyMap {
    svc::PropertyMap {
        properties: properties
            .into_iter()
            .map(|(name, value)| svc::Property {
                name: name.to_string(),
                value,
            })
            .collect(),
    }
}

fn proto_property_map<const N: usize>(
    properties: [(&str, proto::property_value::Kind); N],
) -> proto::PropertyMap {
    proto::PropertyMap {
        properties: properties
            .into_iter()
            .map(|(name, kind)| proto::Property {
                name: name.to_string(),
                value: Some(proto::PropertyValue { kind: Some(kind) }),
            })
            .collect(),
    }
}
