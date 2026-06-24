use std::net::SocketAddr;
use std::sync::Arc;

use grm_service_api::{
    ApplicationAuthenticator, AuthenticationError, AuthorizationDecision, AuthorizationPolicy,
    AuthorizationReason, GrpcWorkspaceService, PolicyEvaluationError, Principal, SecurityAction,
    SecurityRequestContext, SecurityResourceKind, ServiceSecurityConfig, TransportPeer, proto,
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::metadata::MetadataMap;
use tonic::transport::{Channel, Server};
use tonic::{Code, Request};

struct FixedAuthenticator;

impl ApplicationAuthenticator for FixedAuthenticator {
    fn authenticate(
        &self,
        _transport_peer: &TransportPeer,
        _metadata: &MetadataMap,
    ) -> Result<Option<Principal>, AuthenticationError> {
        Ok(Some(Principal {
            issuer: "test-service".into(),
            subject: "test-principal".into(),
            authentication_method: "server-test-fixture".into(),
        }))
    }
}

struct AllowPolicy;

impl AuthorizationPolicy for AllowPolicy {
    fn evaluate(
        &self,
        _context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        Ok(AuthorizationDecision::Allow {
            reason: AuthorizationReason::ExplicitPolicyAllow,
        })
    }
}

struct DenyActionPolicy(SecurityAction);

impl AuthorizationPolicy for DenyActionPolicy {
    fn evaluate(
        &self,
        context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        if context
            .operations
            .iter()
            .any(|operation| operation.action == self.0)
        {
            Ok(AuthorizationDecision::Deny {
                reason: AuthorizationReason::NoMatchingPermission,
            })
        } else {
            Ok(AuthorizationDecision::Allow {
                reason: AuthorizationReason::ExplicitPolicyAllow,
            })
        }
    }
}

struct ErrorActionPolicy(SecurityAction);

impl AuthorizationPolicy for ErrorActionPolicy {
    fn evaluate(
        &self,
        context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        if context
            .operations
            .iter()
            .any(|operation| operation.action == self.0)
        {
            Err(PolicyEvaluationError)
        } else {
            Ok(AuthorizationDecision::Allow {
                reason: AuthorizationReason::ExplicitPolicyAllow,
            })
        }
    }
}

struct DenyResourcePolicy {
    action: SecurityAction,
    kind: SecurityResourceKind,
    model: &'static str,
}

impl AuthorizationPolicy for DenyResourcePolicy {
    fn evaluate(
        &self,
        context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        if context.operations.iter().any(|operation| {
            operation.action == self.action
                && operation.resource.kind == self.kind
                && operation.resource.model.as_deref() == Some(self.model)
        }) {
            Ok(AuthorizationDecision::Deny {
                reason: AuthorizationReason::NoMatchingPermission,
            })
        } else {
            Ok(AuthorizationDecision::Allow {
                reason: AuthorizationReason::ExplicitPolicyAllow,
            })
        }
    }
}

#[tokio::test]
async fn explicit_anonymous_local_profile_executes_workspace_operations() {
    let (mut client, shutdown, server) =
        start_service(ServiceSecurityConfig::anonymous_local()).await;
    let handle = create_workspace(&mut client).await;

    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(user_model()),
    )
    .await
    .unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn anonymous_local_profile_refuses_public_bind_addresses() {
    let public: SocketAddr = "0.0.0.0:50051".parse().unwrap();
    let loopback: SocketAddr = "127.0.0.1:50051".parse().unwrap();

    assert!(
        ServiceSecurityConfig::anonymous_local()
            .validate_bind_addr(public)
            .is_err()
    );
    assert!(
        ServiceSecurityConfig::anonymous_local()
            .validate_bind_addr(loopback)
            .is_ok()
    );
    assert!(
        ServiceSecurityConfig::secured()
            .validate_bind_addr(public)
            .is_ok()
    );
}

#[tokio::test]
async fn secured_profile_rejects_anonymous_actor_assertion() {
    let (mut client, shutdown, server) = start_service(ServiceSecurityConfig::secured()).await;
    let mut request = Request::new(in_memory_workspace_create_request());
    request
        .metadata_mut()
        .insert("x-grm-actor-id", "claimed-admin".parse().unwrap());

    let denied = client.create_workspace(request).await.unwrap_err();
    assert_eq!(denied.code(), Code::Unauthenticated);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn local_workspace_refs_reject_path_like_ids() {
    let rejected = [
        "",
        "../escape",
        "/absolute",
        r"nested\path",
        ".",
        "name.json",
    ];

    for workspace_id in rejected {
        let temp = tempfile::tempdir().unwrap();
        let (mut client, shutdown, server) =
            start_local_service(temp.path(), ServiceSecurityConfig::anonymous_local()).await;

        let err = client
            .create_workspace(proto::WorkspaceCreateRequest {
                mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
                workspace: Some(proto::WorkspaceRef {
                    id: workspace_id.into(),
                }),
                format: proto::DurabilityFormat::Binary as i32,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument, "{workspace_id:?}");

        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn secured_default_policy_denies_authenticated_principal() {
    let security =
        ServiceSecurityConfig::secured().with_authenticator(Arc::new(FixedAuthenticator));
    let (mut client, shutdown, server) = start_service(security).await;
    let denied = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn secured_profile_rejects_anonymous_workspace_lifecycle_requests() {
    let (mut client, shutdown, server) = start_service(ServiceSecurityConfig::secured()).await;

    let create = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(create.code(), Code::Unauthenticated);

    let open = client
        .open_workspace(proto::WorkspaceOpenRequest {
            snapshot: None,
            workspace: Some(proto::WorkspaceRef {
                id: "claimed-workspace".into(),
            }),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(open.code(), Code::Unauthenticated);

    let close = client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(proto::WorkspaceHandle {
                id: "claimed-handle".into(),
            }),
        })
        .await
        .unwrap_err();
    assert_eq!(close.code(), Code::Unauthenticated);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn denied_workspace_close_keeps_handle_active() {
    let security = secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::WorkspaceClose)));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;

    let denied = client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle.clone()),
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);

    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
    )
    .await
    .unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn secured_denial_does_not_disclose_workspace_handle_existence() {
    let execute_security =
        secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::SchemaInspect)));
    let (mut execute_client, execute_shutdown, execute_server) =
        start_service(execute_security).await;
    let existing = create_workspace(&mut execute_client).await;
    let missing = proto::WorkspaceHandle {
        id: "missing-workspace".into(),
    };

    for handle in [&existing, &missing] {
        let denied = execute(
            &mut execute_client,
            handle,
            proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
        )
        .await
        .unwrap_err();
        assert_eq!(denied.code(), Code::PermissionDenied);
        assert_eq!(denied.message(), "authorization denied");
    }
    execute_shutdown.send(()).unwrap();
    execute_server.await.unwrap().unwrap();

    let close_security =
        secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::WorkspaceClose)));
    let (mut close_client, close_shutdown, close_server) = start_service(close_security).await;
    let existing = create_workspace(&mut close_client).await;
    for handle in [
        existing,
        proto::WorkspaceHandle {
            id: "missing-workspace".into(),
        },
    ] {
        let denied = close_client
            .close_workspace(proto::WorkspaceCloseRequest {
                handle: Some(handle),
            })
            .await
            .unwrap_err();
        assert_eq!(denied.code(), Code::PermissionDenied);
        assert_eq!(denied.message(), "authorization denied");
    }

    close_shutdown.send(()).unwrap();
    close_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn authorized_unknown_handle_returns_not_found_after_policy_allows() {
    let security = secured_with_policy(Arc::new(AllowPolicy));
    let (mut client, shutdown, server) = start_service(security).await;
    let missing = proto::WorkspaceHandle {
        id: "missing-workspace".into(),
    };

    let execute_error = execute(
        &mut client,
        &missing,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
    )
    .await
    .unwrap_err();
    assert_eq!(execute_error.code(), Code::NotFound);

    let close_error = client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(missing),
        })
        .await
        .unwrap_err();
    assert_eq!(close_error.code(), Code::NotFound);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn denied_request_does_not_execute_or_mutate_graph_state() {
    let security = secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::NodeCreate)));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(user_model()),
    )
    .await
    .unwrap();

    let denied = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateNode(user_create("Ada")),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);

    let found = find_users(&mut client, &handle, None).await;
    assert!(found.nodes.is_empty());

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn policy_evaluation_error_fails_closed_before_execution() {
    let security = secured_with_policy(Arc::new(ErrorActionPolicy(SecurityAction::NodeCreate)));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(user_model()),
    )
    .await
    .unwrap();

    let failed = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateNode(user_create("Ada")),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code(), Code::Unavailable);
    assert!(
        find_users(&mut client, &handle, None)
            .await
            .nodes
            .is_empty()
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn batch_authorization_uses_contained_operations_not_client_delete_label() {
    let security = secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::NodeDelete)));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(user_model()),
    )
    .await
    .unwrap();
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateNode(user_create("Ada")),
    )
    .await
    .unwrap();

    let denied = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::ApplyBatch(proto::BatchRequest {
            atomic: true,
            allow_deletes: true,
            response_mode: proto::BatchResponseMode::Detailed as i32,
            ops: vec![
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::NodeCreate(
                        proto::BatchNodeCreate {
                            model: "User".into(),
                            props: Some(properties("name", "Grace")),
                            local_ref: None,
                        },
                    )),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::NodeDelete(
                        proto::NodeDeleteRequest {
                            model: "User".into(),
                            id: 1,
                        },
                    )),
                },
            ],
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(
        find_users(&mut client, &handle, Some(1)).await.nodes.len(),
        1
    );
    assert!(
        find_users(&mut client, &handle, Some(2))
            .await
            .nodes
            .is_empty()
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn secured_batch_limit_returns_resource_exhausted_before_runtime() {
    let security = secured_with_policy(Arc::new(AllowPolicy)).with_max_batch_operations(1);
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;

    let over_limit = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::ApplyBatch(proto::BatchRequest {
            atomic: true,
            allow_deletes: false,
            response_mode: proto::BatchResponseMode::Detailed as i32,
            ops: vec![
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::SchemaDefineNode(user_model())),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::SchemaDefineNode(
                        proto::DefineNodeRequest {
                            name: "Post".into(),
                            id_field: "postId".into(),
                            fields: vec![],
                        },
                    )),
                },
            ],
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(over_limit.code(), Code::ResourceExhausted);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn authorized_request_still_runs_runtime_validation() {
    let security = secured_with_policy(Arc::new(AllowPolicy));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(user_model()),
    )
    .await
    .unwrap();

    let invalid = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateNode(proto::NodeCreateRequest {
            model: "User".into(),
            props: Some(proto::PropertyMap { properties: vec![] }),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(invalid.code(), Code::InvalidArgument);
    assert!(
        find_users(&mut client, &handle, None)
            .await
            .nodes
            .is_empty()
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn query_wrappers_include_underlying_node_read_authorization() {
    let security = secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::NodeRead)));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(user_model()),
    )
    .await
    .unwrap();

    let query = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::Query(node_find_query("User")),
    )
    .await
    .unwrap_err();
    assert_eq!(query.code(), Code::PermissionDenied);

    let profile = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::Profile(proto::ProfileRequest {
            query: Some(node_find_query("User")),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(profile.code(), Code::PermissionDenied);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn query_wrappers_include_underlying_edge_and_traversal_authorization() {
    let edge_security = secured_with_policy(Arc::new(DenyActionPolicy(SecurityAction::EdgeRead)));
    let (mut edge_client, edge_shutdown, edge_server) = start_service(edge_security).await;
    let edge_handle = create_workspace(&mut edge_client).await;
    let edge = execute(
        &mut edge_client,
        &edge_handle,
        proto::runtime_request::Request::Query(edge_find_query("Authored")),
    )
    .await
    .unwrap_err();
    assert_eq!(edge.code(), Code::PermissionDenied);
    edge_shutdown.send(()).unwrap();
    edge_server.await.unwrap().unwrap();

    let traversal_security = secured_with_policy(Arc::new(DenyResourcePolicy {
        action: SecurityAction::EdgeRead,
        kind: SecurityResourceKind::EdgeModel,
        model: "Authored",
    }));
    let (mut traversal_client, traversal_shutdown, traversal_server) =
        start_service(traversal_security).await;
    let traversal_handle = create_workspace(&mut traversal_client).await;
    let traversal = execute(
        &mut traversal_client,
        &traversal_handle,
        proto::runtime_request::Request::Explain(proto::ExplainRequest {
            query: Some(traversal_query("User", "Authored", "Post")),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(traversal.code(), Code::PermissionDenied);
    traversal_shutdown.send(()).unwrap();
    traversal_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn traversal_classifies_destination_models_for_query_and_direct_find() {
    let destination_policy = || {
        Arc::new(DenyResourcePolicy {
            action: SecurityAction::NodeRead,
            kind: SecurityResourceKind::NodeModel,
            model: "Post",
        })
    };

    let (mut query_client, query_shutdown, query_server) =
        start_service(secured_with_policy(destination_policy())).await;
    let query_handle = create_workspace(&mut query_client).await;
    let query = execute(
        &mut query_client,
        &query_handle,
        proto::runtime_request::Request::Query(traversal_query("User", "Authored", "Post")),
    )
    .await
    .unwrap_err();
    assert_eq!(query.code(), Code::PermissionDenied);
    query_shutdown.send(()).unwrap();
    query_server.await.unwrap().unwrap();

    let (mut direct_client, direct_shutdown, direct_server) =
        start_service(secured_with_policy(destination_policy())).await;
    let direct_handle = create_workspace(&mut direct_client).await;
    let direct = execute(
        &mut direct_client,
        &direct_handle,
        proto::runtime_request::Request::FindNodes(node_find_with_traversal(
            "User", "Authored", "Post",
        )),
    )
    .await
    .unwrap_err();
    assert_eq!(direct.code(), Code::PermissionDenied);
    direct_shutdown.send(()).unwrap();
    direct_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn direct_find_traversal_classifies_edge_models() {
    let security = secured_with_policy(Arc::new(DenyResourcePolicy {
        action: SecurityAction::EdgeRead,
        kind: SecurityResourceKind::EdgeModel,
        model: "Authored",
    }));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;

    let denied = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::FindNodes(node_find_with_traversal(
            "User", "Authored", "Post",
        )),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn secured_profile_rejects_implicit_edge_traversal_before_authorization() {
    let security = secured_with_policy(Arc::new(AllowPolicy));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;

    let query = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::Query(implicit_edge_traversal_query("User", "Post")),
    )
    .await
    .unwrap_err();
    assert_eq!(query.code(), Code::InvalidArgument);
    assert_eq!(query.message(), "secured traversal requires edge_model");

    let node_query = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::Query(implicit_edge_node_find_query("User", "Post")),
    )
    .await
    .unwrap_err();
    assert_eq!(node_query.code(), Code::InvalidArgument);
    assert_eq!(
        node_query.message(),
        "secured traversal requires edge_model"
    );

    let direct = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::FindNodes(implicit_edge_node_find("User", "Post")),
    )
    .await
    .unwrap_err();
    assert_eq!(direct.code(), Code::InvalidArgument);
    assert_eq!(direct.message(), "secured traversal requires edge_model");

    let explain = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::Explain(proto::ExplainRequest {
            query: Some(implicit_edge_traversal_query("User", "Post")),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(explain.code(), Code::InvalidArgument);
    assert_eq!(explain.message(), "secured traversal requires edge_model");

    let profile = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::Profile(proto::ProfileRequest {
            query: Some(implicit_edge_traversal_query("User", "Post")),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(profile.code(), Code::InvalidArgument);
    assert_eq!(profile.message(), "secured traversal requires edge_model");

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

fn secured_with_policy(policy: Arc<dyn AuthorizationPolicy>) -> ServiceSecurityConfig {
    ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(FixedAuthenticator))
        .with_policy(policy)
}

async fn start_service(
    security: ServiceSecurityConfig,
) -> (
    proto::grm_service_client::GrmServiceClient<Channel>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = GrpcWorkspaceService::new(security).into_server();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let client = proto::grm_service_client::GrmServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap();
    (client, shutdown_tx, server)
}

async fn start_local_service(
    root: &std::path::Path,
    security: ServiceSecurityConfig,
) -> (
    proto::grm_service_client::GrmServiceClient<Channel>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = GrpcWorkspaceService::with_local_workspace_root(root, security).into_server();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let client = proto::grm_service_client::GrmServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap();
    (client, shutdown_tx, server)
}

async fn create_workspace(
    client: &mut proto::grm_service_client::GrmServiceClient<Channel>,
) -> proto::WorkspaceHandle {
    client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap()
        .into_inner()
        .handle
        .unwrap()
}

fn in_memory_workspace_create_request() -> proto::WorkspaceCreateRequest {
    proto::WorkspaceCreateRequest {
        mode: proto::WorkspaceCreateMode::InMemory as i32,
        workspace: None,
        format: proto::DurabilityFormat::Json as i32,
    }
}

async fn execute(
    client: &mut proto::grm_service_client::GrmServiceClient<Channel>,
    handle: &proto::WorkspaceHandle,
    request: proto::runtime_request::Request,
) -> Result<proto::WorkspaceRuntimeResponse, tonic::Status> {
    client
        .execute_workspace(workspace_request(handle, request))
        .await
        .map(|response| response.into_inner())
}

fn workspace_request(
    handle: &proto::WorkspaceHandle,
    request: proto::runtime_request::Request,
) -> Request<proto::WorkspaceRuntimeRequest> {
    Request::new(proto::WorkspaceRuntimeRequest {
        handle: Some(handle.clone()),
        request: Some(proto::RuntimeRequest {
            request: Some(request),
        }),
    })
}

async fn find_users(
    client: &mut proto::grm_service_client::GrmServiceClient<Channel>,
    handle: &proto::WorkspaceHandle,
    id: Option<i64>,
) -> proto::NodeFindResponse {
    let response = execute(
        client,
        handle,
        proto::runtime_request::Request::FindNodes(proto::NodeFindRequest {
            model: "User".into(),
            predicates: vec![],
            end_predicates: vec![],
            edge_predicates: vec![],
            traversals: vec![],
            order: vec![],
            limit: None,
            offset: None,
            id,
            return_mode: None,
        }),
    )
    .await
    .unwrap();
    match response.response.unwrap().response.unwrap() {
        proto::runtime_response::Response::FindNodes(response) => response,
        other => panic!("expected FindNodes response, got {other:?}"),
    }
}

fn user_model() -> proto::DefineNodeRequest {
    proto::DefineNodeRequest {
        name: "User".into(),
        id_field: "userId".into(),
        fields: vec![proto::FieldSpec {
            name: "name".into(),
            value_type: proto::FieldValueType::String as i32,
            required: true,
        }],
    }
}

fn user_create(name: &str) -> proto::NodeCreateRequest {
    proto::NodeCreateRequest {
        model: "User".into(),
        props: Some(properties("name", name)),
    }
}

fn node_find_query(model: &str) -> proto::QueryRequest {
    proto::QueryRequest {
        query: Some(proto::query_request::Query::NodeFind(node_find_shape(
            model,
        ))),
    }
}

fn edge_find_query(model: &str) -> proto::QueryRequest {
    proto::QueryRequest {
        query: Some(proto::query_request::Query::EdgeFind(
            proto::EdgeFindShape {
                model: model.into(),
                predicates: vec![],
                order: vec![],
                limit: None,
                offset: None,
                id: None,
                from: None,
                to: None,
            },
        )),
    }
}

fn traversal_query(root_model: &str, edge_model: &str, end_model: &str) -> proto::QueryRequest {
    proto::QueryRequest {
        query: Some(proto::query_request::Query::Traversal(
            proto::TraversalRequest {
                root: Some(node_find_shape_with_traversal(
                    root_model, edge_model, end_model,
                )),
            },
        )),
    }
}

fn implicit_edge_traversal_query(root_model: &str, end_model: &str) -> proto::QueryRequest {
    proto::QueryRequest {
        query: Some(proto::query_request::Query::Traversal(
            proto::TraversalRequest {
                root: Some(node_find_shape_with_steps(
                    root_model,
                    vec![implicit_edge_traversal_step(end_model)],
                )),
            },
        )),
    }
}

fn implicit_edge_node_find_query(root_model: &str, end_model: &str) -> proto::QueryRequest {
    proto::QueryRequest {
        query: Some(proto::query_request::Query::NodeFind(
            node_find_shape_with_steps(root_model, vec![implicit_edge_traversal_step(end_model)]),
        )),
    }
}

fn implicit_edge_node_find(root_model: &str, end_model: &str) -> proto::NodeFindRequest {
    proto::NodeFindRequest {
        model: root_model.into(),
        predicates: vec![],
        end_predicates: vec![],
        edge_predicates: vec![],
        traversals: vec![implicit_edge_traversal_step(end_model)],
        order: vec![],
        limit: None,
        offset: None,
        id: None,
        return_mode: None,
    }
}

fn node_find_with_traversal(
    root_model: &str,
    edge_model: &str,
    end_model: &str,
) -> proto::NodeFindRequest {
    proto::NodeFindRequest {
        model: root_model.into(),
        predicates: vec![],
        end_predicates: vec![],
        edge_predicates: vec![],
        traversals: vec![traversal_step(edge_model, end_model)],
        order: vec![],
        limit: None,
        offset: None,
        id: None,
        return_mode: None,
    }
}

fn node_find_shape_with_traversal(
    root_model: &str,
    edge_model: &str,
    end_model: &str,
) -> proto::NodeFindShape {
    node_find_shape_with_steps(root_model, vec![traversal_step(edge_model, end_model)])
}

fn node_find_shape_with_steps(
    root_model: &str,
    traversals: Vec<proto::TraversalStep>,
) -> proto::NodeFindShape {
    let mut shape = node_find_shape(root_model);
    shape.traversals = traversals;
    shape
}

fn traversal_step(edge_model: &str, end_model: &str) -> proto::TraversalStep {
    proto::TraversalStep {
        direction: proto::TraversalDirection::Out as i32,
        edge_model: Some(edge_model.into()),
        end_model: end_model.into(),
    }
}

fn implicit_edge_traversal_step(end_model: &str) -> proto::TraversalStep {
    proto::TraversalStep {
        direction: proto::TraversalDirection::Out as i32,
        edge_model: None,
        end_model: end_model.into(),
    }
}

fn node_find_shape(model: &str) -> proto::NodeFindShape {
    proto::NodeFindShape {
        model: model.into(),
        predicates: vec![],
        end_predicates: vec![],
        edge_predicates: vec![],
        traversals: vec![],
        order: vec![],
        limit: None,
        offset: None,
        id: None,
        return_mode: None,
    }
}

fn properties(name: &str, value: &str) -> proto::PropertyMap {
    proto::PropertyMap {
        properties: vec![proto::Property {
            name: name.into(),
            value: Some(proto::PropertyValue {
                kind: Some(proto::property_value::Kind::StringValue(value.into())),
            }),
        }],
    }
}
