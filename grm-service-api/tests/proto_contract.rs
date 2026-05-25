use std::fs;

use grm_rs::{
    BatchRequest, DurableOperation, EdgeRequest, NodeRequest, QueryRequest, RuntimeDispatchOutcome,
    RuntimeRequest, RuntimeResponse, SchemaRequest,
};
use grm_service_api as svc;
use grm_service_api::{PROTO_FILES, proto_files};
use serde_json::json;

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
    let files = grm_service_api::proto_files().collect::<Vec<_>>();
    let includes = [grm_service_api::proto_root()];

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
        response: RuntimeResponse::Node(grm_rs::NodeResponse::Delete(grm_rs::RuntimeDelete {
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
    let mut state = grm_rs::SessionState::new();

    let generated = svc::proto::DefineNodeRequest {
        name: "User".into(),
        id_field: "userId".into(),
        fields: vec![svc::proto::FieldSpec {
            name: "name".into(),
            value_type: svc::proto::FieldValueType::String as i32,
            required: true,
        }],
    };
    let request = svc::ServiceRequest::DefineNode(generated.try_into().unwrap());

    let outcome = request.execute(&mut state).await.unwrap();

    assert!(matches!(
        outcome.response,
        RuntimeResponse::Schema(grm_rs::SchemaResponse::DefineNode(model))
            if model.name == "User" && model.fields[0].name == "name"
    ));
    assert!(matches!(
        outcome.durable_ops.as_slice(),
        [DurableOperation::RegisterNodeModel { model }] if model.name == "User"
    ));
}

#[tokio::test]
async fn generated_proto_batch_request_executes_existing_runtime_batch_path() {
    let mut state = grm_rs::SessionState::new();

    let generated = svc::proto::BatchRequest {
        atomic: true,
        allow_deletes: false,
        response_mode: svc::proto::BatchResponseMode::Detailed as i32,
        ops: vec![
            svc::proto::BatchOperation {
                op: Some(svc::proto::batch_operation::Op::SchemaDefineNode(
                    svc::proto::DefineNodeRequest {
                        name: "User".into(),
                        id_field: "userId".into(),
                        fields: vec![svc::proto::FieldSpec {
                            name: "name".into(),
                            value_type: svc::proto::FieldValueType::String as i32,
                            required: true,
                        }],
                    },
                )),
            },
            svc::proto::BatchOperation {
                op: Some(svc::proto::batch_operation::Op::NodeCreate(
                    svc::proto::BatchNodeCreate {
                        model: "User".into(),
                        props: Some(proto_property_map([(
                            "name",
                            svc::proto::property_value::Kind::StringValue("Ada".into()),
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
    let mut state = grm_rs::SessionState::new();
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
    let mut state = grm_rs::SessionState::new();

    let err = svc::ServiceRequest::Explain(svc::ExplainRequest {
        query: svc::QueryRequest {
            query: svc::Query::NodeFind(svc::NodeFindShape {
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
            }),
        },
    })
    .execute(&mut state)
    .await
    .unwrap_err();

    assert!(matches!(err, grm_rs::GrmError::NotSupported(_)));
    assert!(err.to_string().contains("explain requests yet"));
}

#[tokio::test]
async fn in_process_workspace_service_executes_generated_requests_against_handle() {
    let mut service = svc::InProcessWorkspaceService::new();
    let created = service
        .create_workspace(
            svc::proto::WorkspaceCreateRequest {
                mode: svc::proto::WorkspaceCreateMode::InMemory as i32,
            }
            .try_into()
            .unwrap(),
        )
        .unwrap();
    let generated_created: svc::proto::WorkspaceCreateResponse = created.clone().into();
    assert_eq!(generated_created.handle.unwrap().id, created.handle.id);
    assert!(!created.handle.id.is_empty());

    let generated_schema = svc::proto::DefineNodeRequest {
        name: "User".into(),
        id_field: "userId".into(),
        fields: vec![svc::proto::FieldSpec {
            name: "name".into(),
            value_type: svc::proto::FieldValueType::String as i32,
            required: true,
        }],
    };
    let schema_response = service
        .execute_runtime(
            svc::proto::WorkspaceRuntimeRequest {
                handle: Some(created.handle.clone().into()),
                request: Some(svc::proto::RuntimeRequest {
                    request: Some(svc::proto::runtime_request::Request::DefineNode(
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
    let generated_schema_response: svc::proto::WorkspaceRuntimeResponse =
        schema_response.try_into().unwrap();
    assert!(matches!(
        generated_schema_response
            .response
            .and_then(|response| response.response),
        Some(svc::proto::runtime_response::Response::DefineNode(response))
            if response.model.as_ref().unwrap().name == "User"
                && response.durability.as_ref().unwrap().has_durable_mutation
    ));

    let generated_batch = svc::proto::BatchRequest {
        atomic: true,
        allow_deletes: false,
        response_mode: svc::proto::BatchResponseMode::Detailed as i32,
        ops: vec![svc::proto::BatchOperation {
            op: Some(svc::proto::batch_operation::Op::NodeCreate(
                svc::proto::BatchNodeCreate {
                    model: "User".into(),
                    props: Some(proto_property_map([(
                        "name",
                        svc::proto::property_value::Kind::StringValue("Ada".into()),
                    )])),
                    local_ref: Some("ada".into()),
                },
            )),
        }],
    };
    let batch_response = service
        .execute_runtime(
            svc::proto::WorkspaceRuntimeRequest {
                handle: Some(created.handle.clone().into()),
                request: Some(svc::proto::RuntimeRequest {
                    request: Some(svc::proto::runtime_request::Request::ApplyBatch(
                        generated_batch,
                    )),
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
    let generated_batch_response: svc::proto::WorkspaceRuntimeResponse =
        batch_response.try_into().unwrap();
    assert!(matches!(
        generated_batch_response
            .response
            .and_then(|response| response.response),
        Some(svc::proto::runtime_response::Response::ApplyBatch(response))
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
    let snapshot = service
        .save_workspace_to_local_snapshot(
            &created.handle,
            svc::LocalWorkspaceSnapshotRequest {
                format: svc::DurabilityFormat::Json,
                path,
            },
        )
        .unwrap();
    let closed = service
        .close_workspace(
            svc::proto::WorkspaceCloseRequest {
                handle: Some(created.handle.clone().into()),
            }
            .try_into()
            .unwrap(),
        )
        .unwrap();
    let generated_closed: svc::proto::WorkspaceCloseResponse = closed.into();
    assert_eq!(generated_closed.handle.unwrap().id, created.handle.id);

    let opened = service
        .open_workspace(
            svc::proto::WorkspaceOpenRequest {
                snapshot: Some(snapshot.into()),
                format: svc::proto::DurabilityFormat::Json as i32,
            }
            .try_into()
            .unwrap(),
        )
        .unwrap();
    let generated_opened: svc::proto::WorkspaceOpenResponse = opened.clone().into();
    assert_eq!(generated_opened.handle.unwrap().id, opened.handle.id);
    assert_ne!(opened.handle, created.handle);

    let reopened = service.workspace(&opened.handle).unwrap();
    let model = reopened.state().model("User").unwrap();
    assert_eq!(model.origin, grm_rs::RuntimeSchemaOrigin::Declared);
    assert!(model.field("email").is_some());
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

fn read_proto(relative: &str) -> String {
    fs::read_to_string(grm_service_api::proto_root().join(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

fn all_proto_text() -> String {
    proto_files()
        .map(|file| fs::read_to_string(file).expect("proto file should be readable"))
        .collect::<Vec<_>>()
        .join("\n")
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

async fn define_user_post_schema(state: &mut grm_rs::SessionState) {
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
    properties: [(&str, svc::proto::property_value::Kind); N],
) -> svc::proto::PropertyMap {
    svc::proto::PropertyMap {
        properties: properties
            .into_iter()
            .map(|(name, kind)| svc::proto::Property {
                name: name.to_string(),
                value: Some(svc::proto::PropertyValue { kind: Some(kind) }),
            })
            .collect(),
    }
}
