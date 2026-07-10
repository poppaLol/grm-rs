use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use grm_service_api::{
    ApplicationAuthenticator, AuditStoreFailurePoint, AuditStoreInstrumentation,
    AuthenticationError, AuthorizationDecision, AuthorizationPolicy, AuthorizationReason,
    BoundedSecurityAuditSink, DurableSecurityAuditStore, GrpcWorkspaceService, Permission,
    PermissionAssignment, PermissionRole, PermissionScope, PermissionTableConfig,
    PermissionTablePolicy, PolicyEvaluationError, Principal, ResourceSelector,
    RolePermissionAssignment, RolePermissionTableConfig, SecurityAction, SecurityAuditDecision,
    SecurityAuditDeliveryOutcome, SecurityAuditDurabilityOutcome, SecurityAuditEvent,
    SecurityAuditMode, SecurityAuditReason, SecurityAuditRuntimeOutcome, SecurityAuditSink,
    SecurityAuditSinkError, SecurityAuditSinkHealth, SecurityAuditStage, SecurityRequestContext,
    SecurityResourceKind, ServiceSecurityConfig, TransportPeer, proto,
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

struct PrincipalAuthenticator(Principal);

impl ApplicationAuthenticator for PrincipalAuthenticator {
    fn authenticate(
        &self,
        _transport_peer: &TransportPeer,
        _metadata: &MetadataMap,
    ) -> Result<Option<Principal>, AuthenticationError> {
        Ok(Some(self.0.clone()))
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

struct PolicyVersionAssertingPolicy {
    expected: &'static str,
}

impl AuthorizationPolicy for PolicyVersionAssertingPolicy {
    fn policy_version(&self) -> Option<&str> {
        Some(self.expected)
    }

    fn evaluate(
        &self,
        context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        assert_eq!(context.policy_version.as_deref(), Some(self.expected));
        Ok(AuthorizationDecision::Allow {
            reason: AuthorizationReason::ExplicitPolicyAllow,
        })
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
        ServiceSecurityConfig::docker_local_insecure()
            .validate_bind_addr(public)
            .is_ok()
    );
    assert!(
        ServiceSecurityConfig::secured()
            .validate_bind_addr(public)
            .is_ok()
    );
}

#[tokio::test]
async fn authorization_context_carries_policy_version() {
    let security = secured_with_policy(Arc::new(PolicyVersionAssertingPolicy {
        expected: "asserted-policy-v1",
    }));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
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
async fn permission_table_gives_workload_service_and_user_principals_same_semantics() {
    for kind in ["workload", "service", "user"] {
        let principal = principal(kind);
        let security = secured_with_table(
            principal.clone(),
            vec![
                assignment(
                    principal.clone(),
                    PermissionScope::Service,
                    vec![permission(
                        SecurityAction::WorkspaceCreate,
                        ResourceSelector::Service,
                    )],
                ),
                assignment(
                    principal,
                    PermissionScope::DeploymentLocalAllWorkspaces,
                    vec![permission(
                        SecurityAction::SchemaInspect,
                        ResourceSelector::Workspace,
                    )],
                ),
            ],
        );
        let (mut client, shutdown, server) = start_service(security).await;
        let handle = create_workspace(&mut client).await;
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
}

#[tokio::test]
async fn permission_table_denies_wrong_principal_and_wrong_workspace() {
    let allowed = principal("allowed");
    let wrong = principal("wrong");
    let security = secured_with_table(
        wrong,
        vec![assignment(
            allowed.clone(),
            PermissionScope::Service,
            vec![permission(
                SecurityAction::WorkspaceCreate,
                ResourceSelector::Service,
            )],
        )],
    );
    let (mut client, shutdown, server) = start_service(security).await;
    let denied = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();

    let security = secured_with_table(
        allowed.clone(),
        vec![
            assignment(
                allowed.clone(),
                PermissionScope::Service,
                vec![permission(
                    SecurityAction::WorkspaceCreate,
                    ResourceSelector::Service,
                )],
            ),
            assignment(
                allowed,
                PermissionScope::Workspace("other-workspace".into()),
                vec![permission(
                    SecurityAction::SchemaInspect,
                    ResourceSelector::Workspace,
                )],
            ),
        ],
    );
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    let denied = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn permission_table_create_uses_service_scope_before_allocation() {
    let temp = tempfile::tempdir().unwrap();
    let principal = principal("creator");
    let security = secured_with_table(
        principal.clone(),
        vec![assignment(
            principal,
            PermissionScope::DeploymentLocalAllWorkspaces,
            vec![permission(
                SecurityAction::WorkspaceCreate,
                ResourceSelector::Workspace,
            )],
        )],
    );
    let (mut client, shutdown, server) = start_local_service(temp.path(), security).await;
    let denied = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
            workspace: Some(proto::WorkspaceRef {
                id: "requested-id".into(),
            }),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert!(!temp.path().join("requested-id.json").exists());
    assert!(!temp.path().join("requested-id.bin").exists());
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn permission_table_open_masks_missing_and_inaccessible_workspaces() {
    let principal = principal("opener");
    let security = secured_with_table(
        principal.clone(),
        vec![assignment(
            principal,
            PermissionScope::Workspace("allowed-workspace".into()),
            vec![permission(
                SecurityAction::WorkspaceOpen,
                ResourceSelector::Workspace,
            )],
        )],
    );
    let (mut client, shutdown, server) = start_service(security).await;
    let inaccessible = client
        .open_workspace(proto::WorkspaceOpenRequest {
            snapshot: None,
            workspace: Some(proto::WorkspaceRef {
                id: "denied-workspace".into(),
            }),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(inaccessible.code(), Code::PermissionDenied);
    let missing_allowed = client
        .open_workspace(proto::WorkspaceOpenRequest {
            snapshot: None,
            workspace: Some(proto::WorkspaceRef {
                id: "allowed-workspace".into(),
            }),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(missing_allowed.code(), Code::InvalidArgument);
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
async fn permission_table_uses_exact_node_and_edge_model_selectors() {
    let principal = principal("models");
    let security = secured_with_table(
        principal.clone(),
        vec![
            service_create_assignment(principal.clone()),
            assignment(
                principal,
                PermissionScope::DeploymentLocalAllWorkspaces,
                vec![
                    permission(SecurityAction::SchemaDefine, ResourceSelector::AnyNodeModel),
                    permission(SecurityAction::SchemaDefine, ResourceSelector::AnyEdgeModel),
                    permission(
                        SecurityAction::NodeCreate,
                        ResourceSelector::NodeModel("User".into()),
                    ),
                    permission(
                        SecurityAction::EdgeCreate,
                        ResourceSelector::EdgeModel("Authored".into()),
                    ),
                ],
            ),
        ],
    );
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
        proto::runtime_request::Request::DefineNode(post_model()),
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
    let denied_node = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateNode(proto::NodeCreateRequest {
            model: "Post".into(),
            props: Some(properties("title", "Denied")),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(denied_node.code(), Code::PermissionDenied);
    let denied_edge = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::CreateEdge(proto::EdgeCreateRequest {
            model: "Liked".into(),
            from: 1,
            to: 1,
            props: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(denied_edge.code(), Code::PermissionDenied);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn permission_table_requires_batch_wrapper_and_contained_permissions() {
    let principal = principal("batch");
    let batch_request = proto::runtime_request::Request::ApplyBatch(proto::BatchRequest {
        atomic: true,
        allow_deletes: false,
        response_mode: proto::BatchResponseMode::Detailed as i32,
        ops: vec![proto::BatchOperation {
            op: Some(proto::batch_operation::Op::NodeCreate(
                proto::BatchNodeCreate {
                    model: "User".into(),
                    props: Some(properties("name", "Ada")),
                    local_ref: None,
                },
            )),
        }],
    });

    let missing_wrapper = secured_with_table(
        principal.clone(),
        vec![
            service_create_assignment(principal.clone()),
            assignment(
                principal.clone(),
                PermissionScope::DeploymentLocalAllWorkspaces,
                vec![permission(
                    SecurityAction::NodeCreate,
                    ResourceSelector::NodeModel("User".into()),
                )],
            ),
        ],
    );
    let (mut client, shutdown, server) = start_service(missing_wrapper).await;
    let handle = create_workspace(&mut client).await;
    let denied = execute(&mut client, &handle, batch_request.clone())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();

    let missing_contained = secured_with_table(
        principal.clone(),
        vec![
            service_create_assignment(principal.clone()),
            assignment(
                principal,
                PermissionScope::DeploymentLocalAllWorkspaces,
                vec![permission(
                    SecurityAction::BatchApply,
                    ResourceSelector::OperationFamily,
                )],
            ),
        ],
    );
    let (mut client, shutdown, server) = start_service(missing_contained).await;
    let handle = create_workspace(&mut client).await;
    let denied = execute(&mut client, &handle, batch_request)
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn permission_table_requires_query_explain_profile_wrappers_and_underlying_resources() {
    for wrapper_action in [
        SecurityAction::Query,
        SecurityAction::Explain,
        SecurityAction::Profile,
    ] {
        let principal = principal("query");
        let request = wrapper_request(wrapper_action, node_find_query("User"));
        let missing_wrapper = secured_with_table(
            principal.clone(),
            vec![
                service_create_assignment(principal.clone()),
                assignment(
                    principal.clone(),
                    PermissionScope::DeploymentLocalAllWorkspaces,
                    vec![permission(
                        SecurityAction::NodeRead,
                        ResourceSelector::NodeModel("User".into()),
                    )],
                ),
            ],
        );
        let (mut client, shutdown, server) = start_service(missing_wrapper).await;
        let handle = create_workspace(&mut client).await;
        let denied = execute(&mut client, &handle, request.clone())
            .await
            .unwrap_err();
        assert_eq!(denied.code(), Code::PermissionDenied);
        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();

        let missing_underlying = secured_with_table(
            principal.clone(),
            vec![
                service_create_assignment(principal.clone()),
                assignment(
                    principal,
                    PermissionScope::DeploymentLocalAllWorkspaces,
                    vec![permission(
                        wrapper_action,
                        ResourceSelector::OperationFamily,
                    )],
                ),
            ],
        );
        let (mut client, shutdown, server) = start_service(missing_underlying).await;
        let handle = create_workspace(&mut client).await;
        let denied = execute(&mut client, &handle, request).await.unwrap_err();
        assert_eq!(denied.code(), Code::PermissionDenied);
        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn permission_table_save_load_export_import_use_distinct_permissions() {
    for (allowed_action, request) in [
        (
            SecurityAction::WorkspaceSave,
            proto::runtime_request::Request::Save(proto::SaveRequest {
                format: proto::DurabilityFormat::Json as i32,
                requested_snapshot_id: None,
            }),
        ),
        (
            SecurityAction::WorkspaceLoad,
            proto::runtime_request::Request::Load(proto::LoadRequest {
                format: proto::DurabilityFormat::Json as i32,
                snapshot: Some(snapshot("snap")),
            }),
        ),
        (
            SecurityAction::WorkspaceExport,
            proto::runtime_request::Request::Export(proto::ExportRequest {
                snapshot: Some(snapshot("snap")),
            }),
        ),
        (
            SecurityAction::WorkspaceImport,
            proto::runtime_request::Request::Import(proto::ImportRequest {
                document: b"{}".to_vec(),
                format: proto::DurabilityFormat::Json as i32,
            }),
        ),
    ] {
        let principal = principal("durability");
        let denied_security = secured_with_table(
            principal.clone(),
            vec![
                service_create_assignment(principal.clone()),
                assignment(
                    principal.clone(),
                    PermissionScope::DeploymentLocalAllWorkspaces,
                    vec![permission(
                        SecurityAction::WorkspaceInspect,
                        ResourceSelector::Workspace,
                    )],
                ),
            ],
        );
        let (mut client, shutdown, server) = start_service(denied_security).await;
        let handle = create_workspace(&mut client).await;
        let denied = execute(&mut client, &handle, request.clone())
            .await
            .unwrap_err();
        assert_eq!(denied.code(), Code::PermissionDenied);
        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();

        let allowed_security = secured_with_table(
            principal.clone(),
            vec![
                service_create_assignment(principal.clone()),
                assignment(
                    principal,
                    PermissionScope::DeploymentLocalAllWorkspaces,
                    vec![permission(
                        allowed_action,
                        ResourceSelector::WorkspaceArtifact,
                    )],
                ),
            ],
        );
        let (mut client, shutdown, server) = start_service(allowed_security).await;
        let handle = create_workspace(&mut client).await;
        let unsupported = execute(&mut client, &handle, request).await.unwrap_err();
        assert_eq!(unsupported.code(), Code::Unimplemented);
        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn permission_table_load_failure_and_evaluation_failure_fail_closed_before_effects() {
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(FixedAuthenticator))
        .with_policy_load_failure();
    let (mut client, shutdown, server) = start_service(security).await;
    let failed = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(failed.code(), Code::Unavailable);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();

    let security =
        secured_with_policy(Arc::new(ErrorActionPolicy(SecurityAction::WorkspaceCreate)));
    let (mut client, shutdown, server) = start_service(security).await;
    let failed = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(failed.code(), Code::Unavailable);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn permission_table_does_not_grant_authority_from_authentication_method() {
    let allowed = Principal {
        issuer: "issuer".into(),
        subject: "allowed".into(),
        authentication_method: "password".into(),
    };
    let wrong_same_method = Principal {
        issuer: "issuer".into(),
        subject: "wrong".into(),
        authentication_method: "password".into(),
    };
    let security = secured_with_table(
        wrong_same_method,
        vec![service_create_assignment(allowed.clone())],
    );
    let (mut client, shutdown, server) = start_service(security).await;
    let denied = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();

    let authenticated_with_different_method = Principal {
        authentication_method: "mtls-certificate".into(),
        ..allowed.clone()
    };
    let security = secured_with_table(
        authenticated_with_different_method,
        vec![service_create_assignment(allowed)],
    );
    let (mut client, shutdown, server) = start_service(security).await;
    create_workspace(&mut client).await;
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn permission_table_expands_deployment_local_roles_to_exact_permissions() {
    let principal = principal("role");
    let policy = PermissionTablePolicy::from_role_config(RolePermissionTableConfig {
        version: "role-policy-v1".into(),
        roles: vec![PermissionRole {
            name: "schema-reader".into(),
            permissions: vec![permission(
                SecurityAction::SchemaInspect,
                ResourceSelector::Workspace,
            )],
        }],
        assignments: vec![
            RolePermissionAssignment {
                principal: principal.clone(),
                scope: PermissionScope::Service,
                permissions: vec![permission(
                    SecurityAction::WorkspaceCreate,
                    ResourceSelector::Service,
                )],
                roles: vec![],
            },
            RolePermissionAssignment {
                principal: principal.clone(),
                scope: PermissionScope::DeploymentLocalAllWorkspaces,
                permissions: vec![],
                roles: vec!["schema-reader".into()],
            },
        ],
    })
    .unwrap();
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(PrincipalAuthenticator(principal)))
        .with_policy(Arc::new(policy));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;
    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
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

#[tokio::test]
async fn security_audit_records_allowed_request_stages_and_service_authored_fields() {
    let audit = BoundedSecurityAuditSink::default_local();
    let security = secured_with_policy(Arc::new(PolicyVersionAssertingPolicy {
        expected: "audit-policy-v1",
    }))
    .with_audit_sink(Arc::new(audit.clone()));
    let (mut client, shutdown, server) = start_service(security).await;
    let handle = create_workspace(&mut client).await;

    execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
    )
    .await
    .unwrap();

    let events = audit.retained_events();
    let request_id = events.iter().map(|event| event.request_id).max().unwrap();
    let request_events: Vec<_> = events
        .iter()
        .filter(|event| event.request_id == request_id)
        .collect();
    assert_eq!(
        stages(&request_events),
        vec![
            SecurityAuditStage::Attempt,
            SecurityAuditStage::Authentication,
            SecurityAuditStage::Authorization,
            SecurityAuditStage::Admission,
            SecurityAuditStage::Runtime,
            SecurityAuditStage::Durability,
            SecurityAuditStage::Delivery,
        ]
    );
    assert!(request_events.windows(2).all(|pair| {
        pair[0].service_sequence < pair[1].service_sequence
            && pair[0].request_id == pair[1].request_id
    }));

    let authorization = event(&request_events, SecurityAuditStage::Authorization);
    assert_eq!(
        authorization.policy_version.as_deref(),
        Some("audit-policy-v1")
    );
    assert_eq!(authorization.decision, SecurityAuditDecision::Allow);
    assert_eq!(
        authorization.operations[0].action,
        SecurityAction::SchemaInspect
    );
    assert_eq!(
        authorization.operations[0].resource_kind,
        SecurityResourceKind::Workspace
    );
    let principal = authorization.authenticated_principal.as_ref().unwrap();
    assert_eq!(principal.issuer, "test-service");
    assert_eq!(principal.subject, "test-principal");
    assert_eq!(principal.authentication_method, "server-test-fixture");

    let runtime = event(&request_events, SecurityAuditStage::Runtime);
    assert_eq!(
        runtime.runtime_outcome,
        SecurityAuditRuntimeOutcome::Succeeded
    );
    let durability = event(&request_events, SecurityAuditStage::Durability);
    assert_eq!(
        durability.durability_outcome,
        SecurityAuditDurabilityOutcome::NotApplicable
    );
    let delivery = event(&request_events, SecurityAuditStage::Delivery);
    assert_eq!(
        delivery.delivery_outcome,
        SecurityAuditDeliveryOutcome::HandedOff
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn security_audit_records_bounded_identity_overflow_without_retaining_oversized_fields() {
    let actor_audit = BoundedSecurityAuditSink::default_local();
    let actor_security =
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(Arc::new(actor_audit.clone()));
    let (mut actor_client, actor_shutdown, actor_server) = start_service(actor_security).await;
    let oversized_actor = "actor".repeat(40);
    let mut actor_request = Request::new(in_memory_workspace_create_request());
    actor_request
        .metadata_mut()
        .insert("x-grm-actor-id", oversized_actor.parse().unwrap());
    let actor_error = actor_client
        .create_workspace(actor_request)
        .await
        .unwrap_err();
    assert_eq!(actor_error.code(), Code::InvalidArgument);
    let actor_events = actor_audit.retained_events();
    let actor_overflow = actor_events
        .iter()
        .find(|event| {
            event.stage == SecurityAuditStage::Authentication
                && event.reason == SecurityAuditReason::ClassificationOverflow
        })
        .unwrap();
    assert!(actor_overflow.asserted_actor.is_none());
    assert!(!format!("{actor_events:?}").contains("actoractoractor"));
    actor_shutdown.send(()).unwrap();
    actor_server.await.unwrap().unwrap();

    let principal_audit = BoundedSecurityAuditSink::default_local();
    let oversized_principal = Principal {
        issuer: "issuer".repeat(40),
        subject: "principal".into(),
        authentication_method: "server-test-fixture".into(),
    };
    let principal_security = secured_with_policy(Arc::new(AllowPolicy))
        .with_authenticator(Arc::new(PrincipalAuthenticator(oversized_principal)))
        .with_audit_sink(Arc::new(principal_audit.clone()));
    let (mut principal_client, principal_shutdown, principal_server) =
        start_service(principal_security).await;
    let principal_error = principal_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(principal_error.code(), Code::InvalidArgument);
    let principal_events = principal_audit.retained_events();
    let principal_overflow = principal_events
        .iter()
        .find(|event| {
            event.stage == SecurityAuditStage::Authentication
                && event.reason == SecurityAuditReason::ClassificationOverflow
        })
        .unwrap();
    assert!(principal_overflow.authenticated_principal.is_none());
    assert!(!format!("{principal_events:?}").contains("issuerissuerissuer"));
    principal_shutdown.send(()).unwrap();
    principal_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn security_audit_lifecycle_durability_reports_only_supported_evidence() {
    let memory_audit = BoundedSecurityAuditSink::default_local();
    let memory_security =
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(Arc::new(memory_audit.clone()));
    let (mut memory_client, memory_shutdown, memory_server) = start_service(memory_security).await;
    let memory_handle = create_workspace(&mut memory_client).await;
    memory_client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(memory_handle),
        })
        .await
        .unwrap();
    let memory_events = memory_audit.retained_events();
    let create_id = memory_events
        .iter()
        .find(|event| {
            event.stage == SecurityAuditStage::Authorization
                && event
                    .operations
                    .iter()
                    .any(|operation| operation.action == SecurityAction::WorkspaceCreate)
        })
        .unwrap()
        .request_id;
    let close_id = memory_events
        .iter()
        .find(|event| {
            event.stage == SecurityAuditStage::Authorization
                && event
                    .operations
                    .iter()
                    .any(|operation| operation.action == SecurityAction::WorkspaceClose)
        })
        .unwrap()
        .request_id;
    assert_eq!(
        durability_for_request(&memory_events, create_id),
        SecurityAuditDurabilityOutcome::NotApplicable
    );
    assert_eq!(
        durability_for_request(&memory_events, close_id),
        SecurityAuditDurabilityOutcome::NotApplicable
    );
    memory_shutdown.send(()).unwrap();
    memory_server.await.unwrap().unwrap();

    let local_audit = BoundedSecurityAuditSink::default_local();
    let temp = tempfile::tempdir().unwrap();
    let (mut local_client, local_shutdown, local_server) = start_local_service(
        temp.path(),
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(Arc::new(local_audit.clone())),
    )
    .await;
    local_client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
            workspace: Some(proto::WorkspaceRef {
                id: "durable-audit".into(),
            }),
            format: proto::DurabilityFormat::Json as i32,
        })
        .await
        .unwrap();
    let local_events = local_audit.retained_events();
    let local_create_id = local_events
        .iter()
        .find(|event| {
            event.stage == SecurityAuditStage::Authorization
                && event
                    .operations
                    .iter()
                    .any(|operation| operation.action == SecurityAction::WorkspaceCreate)
        })
        .unwrap()
        .request_id;
    assert_eq!(
        durability_for_request(&local_events, local_create_id),
        SecurityAuditDurabilityOutcome::Committed
    );
    local_shutdown.send(()).unwrap();
    local_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn security_audit_keeps_asserted_actor_separate_from_authenticated_principal() {
    let audit = BoundedSecurityAuditSink::default_local();
    let security =
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(Arc::new(audit.clone()));
    let (mut client, shutdown, server) = start_service(security).await;
    let mut request = Request::new(in_memory_workspace_create_request());
    request
        .metadata_mut()
        .insert("x-grm-actor-id", "claimed-admin".parse().unwrap());

    client.create_workspace(request).await.unwrap();

    let events = audit.retained_events();
    let authorization = events
        .iter()
        .find(|event| event.stage == SecurityAuditStage::Authorization)
        .unwrap();
    let principal = authorization.authenticated_principal.as_ref().unwrap();
    let asserted = authorization.asserted_actor.as_ref().unwrap();
    assert_eq!(principal.subject, "test-principal");
    assert_eq!(asserted.actor_id, "claimed-admin");
    assert!(!asserted.authenticated);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn security_audit_records_denied_unauthenticated_policy_error_and_malformed_paths() {
    let unauth_audit = BoundedSecurityAuditSink::default_local();
    let (mut unauth_client, unauth_shutdown, unauth_server) = start_service(
        ServiceSecurityConfig::secured().with_audit_sink(Arc::new(unauth_audit.clone())),
    )
    .await;
    let unauth = unauth_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(unauth.code(), Code::Unauthenticated);
    assert_audit_reason(
        &unauth_audit.retained_events(),
        SecurityAuditStage::Authentication,
        SecurityAuditReason::MissingPrincipal,
    );
    unauth_shutdown.send(()).unwrap();
    unauth_server.await.unwrap().unwrap();

    let denied_audit = BoundedSecurityAuditSink::default_local();
    let (mut denied_client, denied_shutdown, denied_server) = start_service(
        ServiceSecurityConfig::secured()
            .with_authenticator(Arc::new(FixedAuthenticator))
            .with_audit_sink(Arc::new(denied_audit.clone())),
    )
    .await;
    let denied = denied_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_audit_reason(
        &denied_audit.retained_events(),
        SecurityAuditStage::Authorization,
        SecurityAuditReason::NoMatchingPermission,
    );
    denied_shutdown.send(()).unwrap();
    denied_server.await.unwrap().unwrap();

    let policy_error_audit = BoundedSecurityAuditSink::default_local();
    let (mut policy_client, policy_shutdown, policy_server) = start_service(
        secured_with_policy(Arc::new(ErrorActionPolicy(SecurityAction::WorkspaceCreate)))
            .with_audit_sink(Arc::new(policy_error_audit.clone())),
    )
    .await;
    let policy_error = policy_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(policy_error.code(), Code::Unavailable);
    assert_audit_reason(
        &policy_error_audit.retained_events(),
        SecurityAuditStage::Authorization,
        SecurityAuditReason::PolicyEvaluationFailed,
    );
    policy_shutdown.send(()).unwrap();
    policy_server.await.unwrap().unwrap();

    let malformed_audit = BoundedSecurityAuditSink::default_local();
    let (mut malformed_client, malformed_shutdown, malformed_server) = start_service(
        secured_with_policy(Arc::new(AllowPolicy))
            .with_audit_sink(Arc::new(malformed_audit.clone())),
    )
    .await;
    let malformed = malformed_client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: None,
            request: None,
        })
        .await
        .unwrap_err();
    assert_eq!(malformed.code(), Code::InvalidArgument);
    assert_audit_reason(
        &malformed_audit.retained_events(),
        SecurityAuditStage::Admission,
        SecurityAuditReason::MalformedRequest,
    );
    malformed_shutdown.send(()).unwrap();
    malformed_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn security_audit_records_limit_and_runtime_failures() {
    let limit_audit = BoundedSecurityAuditSink::default_local();
    let limit_security = secured_with_policy(Arc::new(AllowPolicy))
        .with_max_batch_operations(0)
        .with_audit_sink(Arc::new(limit_audit.clone()));
    let (mut limit_client, limit_shutdown, limit_server) = start_service(limit_security).await;
    let limit_handle = create_workspace(&mut limit_client).await;
    let over_limit = execute(
        &mut limit_client,
        &limit_handle,
        proto::runtime_request::Request::ApplyBatch(proto::BatchRequest {
            atomic: true,
            allow_deletes: false,
            response_mode: proto::BatchResponseMode::Summary as i32,
            ops: vec![proto::BatchOperation {
                op: Some(proto::batch_operation::Op::SchemaDefineNode(user_model())),
            }],
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(over_limit.code(), Code::ResourceExhausted);
    assert_audit_reason(
        &limit_audit.retained_events(),
        SecurityAuditStage::Admission,
        SecurityAuditReason::LimitExceeded,
    );
    limit_shutdown.send(()).unwrap();
    limit_server.await.unwrap().unwrap();

    let runtime_audit = BoundedSecurityAuditSink::default_local();
    let (mut runtime_client, runtime_shutdown, runtime_server) = start_service(
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(Arc::new(runtime_audit.clone())),
    )
    .await;
    let runtime_handle = create_workspace(&mut runtime_client).await;
    let runtime_failure = execute(
        &mut runtime_client,
        &runtime_handle,
        proto::runtime_request::Request::CreateNode(user_create("value-before-schema")),
    )
    .await
    .unwrap_err();
    assert_eq!(runtime_failure.code(), Code::InvalidArgument);
    let events = runtime_audit.retained_events();
    let runtime = events
        .iter()
        .rev()
        .find(|event| event.stage == SecurityAuditStage::Runtime)
        .unwrap();
    assert_eq!(runtime.reason, SecurityAuditReason::RuntimeFailed);
    assert_eq!(runtime.runtime_outcome, SecurityAuditRuntimeOutcome::Failed);
    assert_eq!(
        runtime.durability_outcome,
        SecurityAuditDurabilityOutcome::NotCommitted
    );
    runtime_shutdown.send(()).unwrap();
    runtime_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn security_audit_redacts_values_and_enforces_bounds_and_retention() {
    let audit = BoundedSecurityAuditSink::new(3, 100_000, Duration::from_secs(60));
    let security =
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(Arc::new(audit.clone()));
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
        proto::runtime_request::Request::CreateNode(user_create("SUPERSECRET")),
    )
    .await
    .unwrap();

    let retained = audit.retained_events();
    assert!(retained.len() <= 3);
    assert!(audit.retained_bytes() <= 100_000);
    let rendered = format!("{retained:?}");
    assert!(!rendered.contains("SUPERSECRET"));
    assert!(!rendered.contains("BEGIN CERTIFICATE"));
    assert!(!rendered.contains("/tmp/"));

    let overflow = execute(
        &mut client,
        &handle,
        proto::runtime_request::Request::DefineNode(proto::DefineNodeRequest {
            name: "X".repeat(129),
            id_field: "id".into(),
            fields: vec![],
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(overflow.code(), Code::InvalidArgument);
    assert_audit_reason(
        &audit.retained_events(),
        SecurityAuditStage::Admission,
        SecurityAuditReason::ClassificationOverflow,
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn secured_profile_audit_backpressure_denies_before_effect_but_anonymous_is_best_effort() {
    let failing = Arc::new(FailingAuditSink::always());
    let (mut secured_client, secured_shutdown, secured_server) =
        start_service(secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(failing.clone()))
            .await;
    let denied = secured_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::Unavailable);
    assert_eq!(failing.attempts(), 1);
    secured_shutdown.send(()).unwrap();
    secured_server.await.unwrap().unwrap();

    let best_effort = Arc::new(FailingAuditSink::always());
    let (mut anonymous_client, anonymous_shutdown, anonymous_server) = start_service(
        ServiceSecurityConfig::anonymous_local().with_audit_sink(best_effort.clone()),
    )
    .await;
    anonymous_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();
    assert!(best_effort.attempts() >= 1);
    anonymous_shutdown.send(()).unwrap();
    anonymous_server.await.unwrap().unwrap();

    let visible = BoundedSecurityAuditSink::default_local();
    let (mut visible_client, visible_shutdown, visible_server) = start_service(
        ServiceSecurityConfig::anonymous_local().with_audit_sink(Arc::new(visible.clone())),
    )
    .await;
    visible_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();
    assert!(
        visible
            .retained_events()
            .iter()
            .all(|event| event.mode == SecurityAuditMode::BestEffort)
    );
    visible_shutdown.send(()).unwrap();
    visible_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn audit_identity_allocation_failure_is_best_effort_only_for_weaker_profiles() {
    for sink in [
        Arc::new(AllocationFailAuditSink::request()),
        Arc::new(AllocationFailAuditSink::event()),
    ] {
        let (mut client, shutdown, server) =
            start_service(ServiceSecurityConfig::anonymous_local().with_audit_sink(sink.clone()))
                .await;
        client
            .create_workspace(in_memory_workspace_create_request())
            .await
            .unwrap();
        assert!(sink.accepted.lock().unwrap().is_empty());
        assert!(sink.event_allocations.load(Ordering::SeqCst) <= 1);
        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();
    }

    for sink in [
        Arc::new(AllocationFailAuditSink::request()),
        Arc::new(AllocationFailAuditSink::event()),
    ] {
        let (mut client, shutdown, server) =
            start_service(secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(sink.clone()))
                .await;
        let denied = client
            .create_workspace(in_memory_workspace_create_request())
            .await
            .unwrap_err();
        assert_eq!(denied.code(), Code::Unavailable);
        assert!(sink.accepted.lock().unwrap().is_empty());
        shutdown.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn durable_sink_failures_preserve_profile_specific_enforcement() {
    let secured_temp = tempfile::tempdir().unwrap();
    let secured_hooks = AuditStoreInstrumentation::default();
    let secured_store = Arc::new(
        DurableSecurityAuditStore::open_with_instrumentation(
            secured_temp.path(),
            32,
            64 * 1024,
            Duration::from_secs(3600),
            secured_hooks.clone(),
        )
        .unwrap(),
    );
    secured_hooks.fail_next(AuditStoreFailurePoint::MetadataTempWrite);
    let (mut secured_client, secured_shutdown, secured_server) = start_service(
        secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(secured_store.clone()),
    )
    .await;
    assert_eq!(
        secured_client
            .create_workspace(in_memory_workspace_create_request())
            .await
            .unwrap_err()
            .code(),
        Code::Unavailable
    );
    assert!(secured_store.retained_events().is_empty());
    secured_shutdown.send(()).unwrap();
    secured_server.await.unwrap().unwrap();

    let best_effort_temp = tempfile::tempdir().unwrap();
    let best_effort_hooks = AuditStoreInstrumentation::default();
    let best_effort_store = Arc::new(
        DurableSecurityAuditStore::open_with_instrumentation(
            best_effort_temp.path(),
            32,
            64 * 1024,
            Duration::from_secs(3600),
            best_effort_hooks.clone(),
        )
        .unwrap(),
    );
    best_effort_hooks.fail_next(AuditStoreFailurePoint::MetadataTempWrite);
    let (mut best_effort_client, best_effort_shutdown, best_effort_server) = start_service(
        ServiceSecurityConfig::anonymous_local().with_audit_sink(best_effort_store.clone()),
    )
    .await;
    best_effort_client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();
    assert!(best_effort_store.retained_events().is_empty());
    best_effort_shutdown.send(()).unwrap();
    best_effort_server.await.unwrap().unwrap();

    let post_effect_temp = tempfile::tempdir().unwrap();
    let post_effect_hooks = AuditStoreInstrumentation::default();
    let post_effect_store = DurableSecurityAuditStore::open_with_instrumentation(
        post_effect_temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        post_effect_hooks.clone(),
    )
    .unwrap();
    let failing = Arc::new(FailDurableAfter {
        store: post_effect_store,
        hooks: post_effect_hooks.clone(),
        fail_after: 4,
        appends: AtomicUsize::new(0),
        failed_once: AtomicBool::new(false),
    });
    let (mut client, shutdown, server) =
        start_service(secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(failing)).await;
    assert!(
        client
            .create_workspace(in_memory_workspace_create_request())
            .await
            .unwrap()
            .into_inner()
            .handle
            .is_some()
    );
    assert!(
        client
            .create_workspace(in_memory_workspace_create_request())
            .await
            .unwrap()
            .into_inner()
            .handle
            .is_some()
    );
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn persistent_durable_recovery_failure_keeps_secured_effects_blocked() {
    let temp = tempfile::tempdir().unwrap();
    let hooks = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        hooks.clone(),
    )
    .unwrap();
    let failing = Arc::new(FailDurableAfter {
        store,
        hooks: hooks.clone(),
        fail_after: 4,
        appends: AtomicUsize::new(0),
        failed_once: AtomicBool::new(false),
    });
    let (mut client, shutdown, server) =
        start_service(secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(failing)).await;
    client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();
    hooks.fail_next(AuditStoreFailurePoint::RecoveryDirectorySync);
    assert_eq!(
        client
            .create_workspace(in_memory_workspace_create_request())
            .await
            .unwrap_err()
            .code(),
        Code::Unavailable
    );
    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn secured_profile_post_effect_audit_failure_degrades_and_blocks_later_effects() {
    let audit = Arc::new(FailAfterAuditSink::new(4));
    let (mut client, shutdown, server) =
        start_service(secured_with_policy(Arc::new(AllowPolicy)).with_audit_sink(audit.clone()))
            .await;

    let first = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap()
        .into_inner();
    assert!(first.handle.is_some());
    let accepted = audit.accepted_events();
    let accepted_refs: Vec<_> = accepted.iter().collect();
    assert_eq!(
        stages(&accepted_refs),
        vec![
            SecurityAuditStage::Attempt,
            SecurityAuditStage::Authentication,
            SecurityAuditStage::Authorization,
            SecurityAuditStage::Admission,
        ]
    );
    assert!(audit.attempts() > 4);

    let blocked = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), Code::Unavailable);
    assert_eq!(
        blocked.message(),
        "authoritative security audit sink unavailable"
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mcp_proxy_identity_is_audit_principal_and_caller_is_only_asserted_context() {
    let audit = BoundedSecurityAuditSink::default_local();
    let backend_principal = Principal {
        issuer: "local-mcp-adapter".into(),
        subject: "configured-mtls-client".into(),
        authentication_method: "mtls-certificate".into(),
    };
    let security = secured_with_policy(Arc::new(AllowPolicy))
        .with_authenticator(Arc::new(PrincipalAuthenticator(backend_principal)))
        .with_audit_sink(Arc::new(audit.clone()));
    let (mut client, shutdown, server) = start_service(security).await;
    let mut request = Request::new(in_memory_workspace_create_request());
    request
        .metadata_mut()
        .insert("x-grm-actor-id", "mcp-caller@example.test".parse().unwrap());

    client.create_workspace(request).await.unwrap();

    let events = audit.retained_events();
    let authz = events
        .iter()
        .find(|event| event.stage == SecurityAuditStage::Authorization)
        .unwrap();
    assert_eq!(
        authz.authenticated_principal.as_ref().unwrap().issuer,
        "local-mcp-adapter"
    );
    assert_eq!(
        authz.authenticated_principal.as_ref().unwrap().subject,
        "configured-mtls-client"
    );
    assert_eq!(
        authz.asserted_actor.as_ref().unwrap().actor_id,
        "mcp-caller@example.test"
    );
    assert!(!authz.asserted_actor.as_ref().unwrap().authenticated);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

fn secured_with_policy(policy: Arc<dyn AuthorizationPolicy>) -> ServiceSecurityConfig {
    ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(FixedAuthenticator))
        .with_policy(policy)
}

fn stages(events: &[&SecurityAuditEvent]) -> Vec<SecurityAuditStage> {
    events.iter().map(|event| event.stage).collect()
}

fn event<'a>(
    events: &'a [&SecurityAuditEvent],
    stage: SecurityAuditStage,
) -> &'a SecurityAuditEvent {
    events.iter().find(|event| event.stage == stage).unwrap()
}

fn assert_audit_reason(
    events: &[SecurityAuditEvent],
    stage: SecurityAuditStage,
    reason: SecurityAuditReason,
) {
    assert!(
        events
            .iter()
            .any(|event| event.stage == stage && event.reason == reason),
        "missing {stage:?}/{reason:?} in {events:#?}"
    );
}

fn durability_for_request(
    events: &[SecurityAuditEvent],
    request_id: u64,
) -> SecurityAuditDurabilityOutcome {
    events
        .iter()
        .find(|event| {
            event.request_id == request_id && event.stage == SecurityAuditStage::Durability
        })
        .unwrap()
        .durability_outcome
}

#[derive(Debug)]
struct FailingAuditSink {
    attempts: AtomicUsize,
}

impl FailingAuditSink {
    fn always() -> Self {
        Self {
            attempts: AtomicUsize::new(0),
        }
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

impl SecurityAuditSink for FailingAuditSink {
    fn append(&self, _event: SecurityAuditEvent) -> Result<(), SecurityAuditSinkError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(SecurityAuditSinkError::Backpressure)
    }

    fn health(&self) -> SecurityAuditSinkHealth {
        SecurityAuditSinkHealth::Degraded
    }
}

#[derive(Debug)]
struct FailAfterAuditSink {
    attempts: AtomicUsize,
    fail_after: usize,
    accepted: Mutex<Vec<SecurityAuditEvent>>,
}

impl FailAfterAuditSink {
    fn new(fail_after: usize) -> Self {
        Self {
            attempts: AtomicUsize::new(0),
            fail_after,
            accepted: Mutex::new(Vec::new()),
        }
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }

    fn accepted_events(&self) -> Vec<SecurityAuditEvent> {
        self.accepted.lock().unwrap().clone()
    }
}

impl SecurityAuditSink for FailAfterAuditSink {
    fn append(&self, event: SecurityAuditEvent) -> Result<(), SecurityAuditSinkError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if attempt > self.fail_after {
            return Err(SecurityAuditSinkError::Backpressure);
        }
        self.accepted.lock().unwrap().push(event);
        Ok(())
    }

    fn health(&self) -> SecurityAuditSinkHealth {
        if self.attempts() > self.fail_after {
            SecurityAuditSinkHealth::Degraded
        } else {
            SecurityAuditSinkHealth::Healthy
        }
    }
}

#[derive(Debug)]
struct AllocationFailAuditSink {
    fail_request: bool,
    fail_event: bool,
    event_allocations: AtomicUsize,
    accepted: Mutex<Vec<SecurityAuditEvent>>,
}

impl AllocationFailAuditSink {
    fn request() -> Self {
        Self {
            fail_request: true,
            fail_event: false,
            event_allocations: AtomicUsize::new(0),
            accepted: Mutex::new(Vec::new()),
        }
    }

    fn event() -> Self {
        Self {
            fail_request: false,
            fail_event: true,
            event_allocations: AtomicUsize::new(0),
            accepted: Mutex::new(Vec::new()),
        }
    }
}

impl SecurityAuditSink for AllocationFailAuditSink {
    fn allocate_request_id(&self) -> Result<u64, SecurityAuditSinkError> {
        if self.fail_request {
            Err(SecurityAuditSinkError::Unavailable)
        } else {
            Ok(100)
        }
    }

    fn allocate_event_identity(&self) -> Result<(u64, u64), SecurityAuditSinkError> {
        self.event_allocations.fetch_add(1, Ordering::SeqCst);
        if self.fail_event {
            Err(SecurityAuditSinkError::Unavailable)
        } else {
            Ok((100, 100))
        }
    }

    fn append(&self, event: SecurityAuditEvent) -> Result<(), SecurityAuditSinkError> {
        self.accepted.lock().unwrap().push(event);
        Ok(())
    }
}

#[derive(Debug)]
struct FailDurableAfter {
    store: DurableSecurityAuditStore,
    hooks: AuditStoreInstrumentation,
    fail_after: usize,
    appends: AtomicUsize,
    failed_once: AtomicBool,
}

impl SecurityAuditSink for FailDurableAfter {
    fn allocate_request_id(&self) -> Result<u64, SecurityAuditSinkError> {
        self.store.allocate_request_id()
    }

    fn allocate_event_identity(&self) -> Result<(u64, u64), SecurityAuditSinkError> {
        self.store.allocate_event_identity()
    }

    fn append(&self, event: SecurityAuditEvent) -> Result<(), SecurityAuditSinkError> {
        let append = self.appends.fetch_add(1, Ordering::SeqCst) + 1;
        if append > self.fail_after && !self.failed_once.swap(true, Ordering::SeqCst) {
            self.hooks.fail_next(AuditStoreFailurePoint::AuditLogWrite);
        }
        self.store.append(event)
    }

    fn health(&self) -> SecurityAuditSinkHealth {
        self.store.health()
    }

    fn try_recover(&self) -> Result<SecurityAuditSinkHealth, SecurityAuditSinkError> {
        self.store.try_recover()
    }
}

fn secured_with_table(
    authenticated_principal: Principal,
    assignments: Vec<PermissionAssignment>,
) -> ServiceSecurityConfig {
    let policy = PermissionTablePolicy::new(PermissionTableConfig {
        version: "test-policy-v1".into(),
        assignments,
    })
    .unwrap();
    ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(PrincipalAuthenticator(authenticated_principal)))
        .with_policy(Arc::new(policy))
}

fn assignment(
    principal: Principal,
    scope: PermissionScope,
    permissions: Vec<Permission>,
) -> PermissionAssignment {
    PermissionAssignment {
        principal,
        scope,
        permissions,
    }
}

fn service_create_assignment(principal: Principal) -> PermissionAssignment {
    assignment(
        principal,
        PermissionScope::Service,
        vec![permission(
            SecurityAction::WorkspaceCreate,
            ResourceSelector::Service,
        )],
    )
}

fn permission(action: SecurityAction, resource: ResourceSelector) -> Permission {
    Permission { action, resource }
}

fn principal(kind: &str) -> Principal {
    Principal {
        issuer: format!("test-{kind}"),
        subject: "principal".into(),
        authentication_method: format!("{kind}-test-fixture"),
    }
}

fn wrapper_request(
    action: SecurityAction,
    query: proto::QueryRequest,
) -> proto::runtime_request::Request {
    match action {
        SecurityAction::Query => proto::runtime_request::Request::Query(query),
        SecurityAction::Explain => {
            proto::runtime_request::Request::Explain(proto::ExplainRequest { query: Some(query) })
        }
        SecurityAction::Profile => {
            proto::runtime_request::Request::Profile(proto::ProfileRequest { query: Some(query) })
        }
        other => panic!("unsupported wrapper action in test: {other:?}"),
    }
}

fn snapshot(id: &str) -> proto::SnapshotHandle {
    proto::SnapshotHandle {
        id: id.into(),
        etag: "etag".into(),
    }
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

fn post_model() -> proto::DefineNodeRequest {
    proto::DefineNodeRequest {
        name: "Post".into(),
        id_field: "postId".into(),
        fields: vec![proto::FieldSpec {
            name: "title".into(),
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
