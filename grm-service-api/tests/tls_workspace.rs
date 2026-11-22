use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use grm_rs::{DefineNodeRequest, FieldSpec, FieldValueType, NodeCreateRequest};
use grm_service_api::{
    AuthorizationDecision, AuthorizationPolicy, AuthorizationReason, BoundedSecurityAuditSink,
    CertificateFingerprint, CertificatePrincipalAuthenticator, CertificatePrincipalMapping,
    DurabilityFormat, GrpcClientTlsOptions, GrpcServerTlsOptions, GrpcWorkspaceClient,
    GrpcWorkspaceMode, GrpcWorkspaceService, MTLS_CERTIFICATE_AUTHENTICATION_METHOD,
    PolicyEvaluationError, Principal, SecurityAuditDecision, SecurityAuditEvent,
    SecurityAuditReason, SecurityAuditSink, SecurityAuditSinkError, SecurityAuditStage,
    SecurityPolicyReloadError, SecurityRequestContext, ServiceSecurityConfig, proto,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Server};
use tonic::{Code, Request};

#[derive(Clone)]
struct ClientIdentity {
    cert: std::path::PathBuf,
    key: std::path::PathBuf,
    fingerprint: CertificateFingerprint,
}

struct AllowExpectedPrincipalPolicy {
    expected: Principal,
}

impl AuthorizationPolicy for AllowExpectedPrincipalPolicy {
    fn evaluate(
        &self,
        context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        if context.authenticated_principal.as_ref() == Some(&self.expected) {
            Ok(AuthorizationDecision::Allow {
                reason: AuthorizationReason::ExplicitPolicyAllow,
            })
        } else {
            Ok(AuthorizationDecision::Deny {
                reason: AuthorizationReason::NoMatchingPermission,
            })
        }
    }
}

struct VersionedAllowExpectedPrincipalPolicy {
    expected: Principal,
    version: &'static str,
}

impl AuthorizationPolicy for VersionedAllowExpectedPrincipalPolicy {
    fn policy_version(&self) -> Option<&str> {
        Some(self.version)
    }

    fn evaluate(
        &self,
        context: &SecurityRequestContext,
    ) -> Result<AuthorizationDecision, PolicyEvaluationError> {
        if context.authenticated_principal.as_ref() == Some(&self.expected) {
            Ok(AuthorizationDecision::Allow {
                reason: AuthorizationReason::ExplicitPolicyAllow,
            })
        } else {
            Ok(AuthorizationDecision::Deny {
                reason: AuthorizationReason::NoMatchingPermission,
            })
        }
    }
}

#[tokio::test]
async fn tls_workspace_service_create_open_execute_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    let ca_cert = temp.path().join("ca.crt");
    let ca_key = temp.path().join("ca.key");
    let cert = temp.path().join("server.crt");
    let key = temp.path().join("server.key");
    let ca = generate_local_ca_cert(&ca_cert, &ca_key);
    generate_localhost_server_cert(&ca, &cert, &key);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        ServiceSecurityConfig::anonymous_local(),
    )
    .into_server();
    let tls = GrpcServerTlsOptions::new(&cert, &key)
        .server_tls_config()
        .unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let client_tls = GrpcClientTlsOptions::new(&ca_cert).with_domain_name("localhost");
    let endpoint = format!("https://127.0.0.1:{port}", port = addr.port());
    let mut created = GrpcWorkspaceClient::connect_with_format_and_tls(
        &endpoint,
        "tls-workspace",
        GrpcWorkspaceMode::Create,
        DurabilityFormat::Binary,
        Some(client_tls.clone()),
    )
    .await
    .unwrap();

    created
        .define_node(DefineNodeRequest {
            name: "User".into(),
            id_field: "userId".into(),
            fields: vec![FieldSpec {
                name: "name".into(),
                value_type: FieldValueType::String,
                required: true,
            }],
        })
        .await
        .unwrap();
    let mut props = BTreeMap::new();
    props.insert("name".into(), serde_json::json!("Ada"));
    let node = created
        .create_node(NodeCreateRequest {
            model: "User".into(),
            props,
        })
        .await
        .unwrap();
    assert_eq!(node.props.get("name").unwrap(), &serde_json::json!("Ada"));
    created.close().await.unwrap();

    let mut opened = GrpcWorkspaceClient::connect_with_format_and_tls(
        &endpoint,
        "tls-workspace",
        GrpcWorkspaceMode::Open,
        DurabilityFormat::Binary,
        Some(client_tls),
    )
    .await
    .unwrap();
    let schema = opened.schema_list().await.unwrap();
    assert_eq!(schema.node_models.len(), 1);
    assert_eq!(schema.node_models[0].name, "User");
    opened.close().await.unwrap();

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mutual_tls_requires_a_client_signed_by_the_trusted_client_ca() {
    let temp = tempfile::tempdir().unwrap();
    let server_ca_cert = temp.path().join("server-ca.crt");
    let server_ca_key = temp.path().join("server-ca.key");
    let server_cert = temp.path().join("server.crt");
    let server_key = temp.path().join("server.key");
    let server_ca = generate_local_ca_cert(&server_ca_cert, &server_ca_key);
    generate_localhost_server_cert(&server_ca, &server_cert, &server_key);

    let client_ca_cert = temp.path().join("client-ca.crt");
    let client_ca_key = temp.path().join("client-ca.key");
    let client_cert = temp.path().join("client.crt");
    let client_key = temp.path().join("client.key");
    let client_ca = generate_local_ca_cert(&client_ca_cert, &client_ca_key);
    generate_client_cert("client", &client_ca, &client_cert, &client_key);

    let rogue_ca_cert = temp.path().join("rogue-ca.crt");
    let rogue_ca_key = temp.path().join("rogue-ca.key");
    let rogue_client_cert = temp.path().join("rogue-client.crt");
    let rogue_client_key = temp.path().join("rogue-client.key");
    let rogue_ca = generate_local_ca_cert(&rogue_ca_cert, &rogue_ca_key);
    generate_client_cert(
        "rogue-client",
        &rogue_ca,
        &rogue_client_cert,
        &rogue_client_key,
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = GrpcWorkspaceService::with_local_workspace_root(
        temp.path(),
        ServiceSecurityConfig::secured(),
    )
    .into_server();
    let tls = GrpcServerTlsOptions::new(&server_cert, &server_key)
        .with_client_ca(&client_ca_cert)
        .server_tls_config()
        .unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(service)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let endpoint = format!("https://127.0.0.1:{port}", port = addr.port());
    let trusted_tls = GrpcClientTlsOptions::new(&server_ca_cert)
        .with_domain_name("localhost")
        .with_identity(&client_cert, &client_key);
    let trusted = GrpcWorkspaceClient::connect_with_format_and_tls(
        &endpoint,
        "mtls-trusted",
        GrpcWorkspaceMode::Create,
        DurabilityFormat::Binary,
        Some(trusted_tls),
    )
    .await;
    let unauthenticated = match trusted {
        Ok(_) => panic!("mTLS transport identity must not grant application authorization"),
        Err(error) => error,
    };
    let grm_service_api::GrpcWorkspaceClientError::Status(status) = unauthenticated else {
        panic!("expected application authentication status");
    };
    assert_eq!(status.code(), tonic::Code::Unauthenticated);

    let missing_identity = GrpcClientTlsOptions::new(&server_ca_cert).with_domain_name("localhost");
    let missing_result = GrpcWorkspaceClient::connect_with_format_and_tls(
        &endpoint,
        "mtls-missing",
        GrpcWorkspaceMode::Create,
        DurabilityFormat::Binary,
        Some(missing_identity),
    )
    .await;
    assert!(
        missing_result.is_err(),
        "client without a certificate must be rejected"
    );

    let rogue_identity = GrpcClientTlsOptions::new(&server_ca_cert)
        .with_domain_name("localhost")
        .with_identity(&rogue_client_cert, &rogue_client_key);
    let rogue_result = GrpcWorkspaceClient::connect_with_format_and_tls(
        &endpoint,
        "mtls-rogue",
        GrpcWorkspaceMode::Create,
        DurabilityFormat::Binary,
        Some(rogue_identity),
    )
    .await;
    assert!(
        rogue_result.is_err(),
        "client signed by an untrusted CA must be rejected"
    );

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mapped_mtls_certificate_establishes_principal_and_still_requires_permission() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.client.fingerprint.clone(),
        principal.clone(),
    )]);
    let security = ServiceSecurityConfig::secured().with_authenticator(Arc::new(authenticator));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    let denied = mtls_client(&endpoint, &fixture, &fixture.client)
        .await
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_public_error_is_redacted(
        &denied,
        &fixture.client.fingerprint,
        &principal,
        &fixture.client.cert,
        &fixture.client.key,
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mapped_mtls_certificate_can_be_authorized_without_actor_metadata_impersonation() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.client.fingerprint.clone(),
        principal.clone(),
    )]);
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(authenticator))
        .with_policy(Arc::new(AllowExpectedPrincipalPolicy {
            expected: principal.clone(),
        }));
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;

    let mut request = Request::new(in_memory_workspace_create_request());
    request.metadata_mut().insert(
        grm_service_api::GRM_ACTOR_ID_METADATA,
        MetadataValue::try_from("claimed-admin").unwrap(),
    );
    let handle = client
        .create_workspace(request)
        .await
        .unwrap()
        .into_inner()
        .handle
        .unwrap();

    client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await
        .unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mapped_mtls_certificate_security_status_reports_safe_principal() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.client.fingerprint.clone(),
        principal.clone(),
    )]);
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(authenticator))
        .with_policy(Arc::new(VersionedAllowExpectedPrincipalPolicy {
            expected: principal.clone(),
            version: "mtls-status-policy-v1",
        }));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    let status = mtls_client(&endpoint, &fixture, &fixture.client)
        .await
        .security_status(proto::SecurityStatusRequest {})
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        status.security_profile,
        proto::SecurityProfile::Secured as i32
    );
    assert_eq!(
        status.identity_status,
        proto::SecurityIdentityStatus::AuthenticatedPrincipal as i32
    );
    assert_eq!(
        status.authentication_method,
        MTLS_CERTIFICATE_AUTHENTICATION_METHOD
    );
    assert_eq!(status.policy_version, "mtls-status-policy-v1");
    let observed = status.principal.expect("principal should be present");
    assert_eq!(observed.issuer, principal.issuer);
    assert_eq!(observed.subject, principal.subject);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn unmapped_mtls_certificate_security_status_fails_without_identity_leakage() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.rotation.fingerprint.clone(),
        principal.clone(),
    )]);
    let security = ServiceSecurityConfig::secured().with_authenticator(Arc::new(authenticator));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    let denied = mtls_client(&endpoint, &fixture, &fixture.client)
        .await
        .security_status(proto::SecurityStatusRequest {})
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::Unauthenticated);
    assert_public_error_is_redacted(
        &denied,
        &fixture.client.fingerprint,
        &principal,
        &fixture.client.cert,
        &fixture.client.key,
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn certificate_authenticator_sets_mtls_authentication_method() {
    let fixture = MtlsFixture::new();
    let configured_principal = test_principal_with_method("misconfigured-provider-label");
    let authenticated_principal =
        test_principal_with_method(MTLS_CERTIFICATE_AUTHENTICATION_METHOD);
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.client.fingerprint.clone(),
        configured_principal,
    )]);
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(authenticator))
        .with_policy(Arc::new(AllowExpectedPrincipalPolicy {
            expected: authenticated_principal,
        }));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    mtls_client(&endpoint, &fixture, &fixture.client)
        .await
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn trusted_but_unmapped_mtls_certificate_fails_application_authentication() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.rotation.fingerprint.clone(),
        principal.clone(),
    )]);
    let security = ServiceSecurityConfig::secured().with_authenticator(Arc::new(authenticator));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    let denied = mtls_client(&endpoint, &fixture, &fixture.client)
        .await
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::Unauthenticated);
    assert_public_error_is_redacted(
        &denied,
        &fixture.client.fingerprint,
        &principal,
        &fixture.client.cert,
        &fixture.client.key,
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn overlapping_mtls_fingerprints_can_map_to_one_principal() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![
        mapping(fixture.client.fingerprint.clone(), principal.clone()),
        mapping(fixture.rotation.fingerprint.clone(), principal.clone()),
    ]);
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(authenticator))
        .with_policy(Arc::new(AllowExpectedPrincipalPolicy {
            expected: principal,
        }));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    mtls_client(&endpoint, &fixture, &fixture.client)
        .await
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();
    mtls_client(&endpoint, &fixture, &fixture.rotation)
        .await
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mapping_removal_reauthenticates_next_rpc_on_existing_tls_channel_and_handle() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator = mapped_authenticator(vec![mapping(
        fixture.client.fingerprint.clone(),
        principal.clone(),
    )]);
    let security = ServiceSecurityConfig::secured()
        .with_authenticator(Arc::new(authenticator.clone()))
        .with_policy(Arc::new(AllowExpectedPrincipalPolicy {
            expected: principal,
        }));
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;
    let handle = client
        .create_workspace(in_memory_workspace_create_request())
        .await
        .unwrap()
        .into_inner()
        .handle
        .unwrap();

    authenticator.replace_mappings([]).unwrap();

    let execute = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::SchemaList(
                    proto::SchemaListRequest {},
                )),
            }),
        })
        .await
        .unwrap_err();
    assert_eq!(execute.code(), Code::Unauthenticated);

    let close = client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await
        .unwrap_err();
    assert_eq!(close.code(), Code::Unauthenticated);

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

/// One principal's grants in a test security.json: service-scope actions on
/// the service resource and all-workspace actions on the workspace resource.
struct PolicyGrant<'a> {
    identity: &'a ClientIdentity,
    subject: &'a str,
    service_actions: &'a [&'a str],
    workspace_actions: &'a [&'a str],
}

fn write_security_config(path: &Path, version: &str, grants: &[PolicyGrant<'_>]) {
    let mut mappings = Vec::new();
    let mut assignments = Vec::new();
    for grant in grants {
        let principal = serde_json::json!({ "issuer": "grm-test", "subject": grant.subject });
        mappings.push(serde_json::json!({
            "fingerprint_sha256": grant.identity.fingerprint.as_hex(),
            "principal": principal,
        }));
        if !grant.service_actions.is_empty() {
            assignments.push(serde_json::json!({
                "principal": principal,
                "scope": { "kind": "service" },
                "permissions": grant.service_actions.iter().map(|action| serde_json::json!({
                    "action": action, "resource": { "kind": "service" }
                })).collect::<Vec<_>>(),
            }));
        }
        if !grant.workspace_actions.is_empty() {
            assignments.push(serde_json::json!({
                "principal": principal,
                "scope": { "kind": "deployment_local_all_workspaces" },
                "permissions": grant.workspace_actions.iter().map(|action| serde_json::json!({
                    "action": action, "resource": { "kind": "workspace" }
                })).collect::<Vec<_>>(),
            }));
        }
    }
    let config = serde_json::json!({
        "certificate_mappings": mappings,
        "permission_table": { "version": version, "assignments": assignments },
    });
    fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

fn reader_grant<'a>(identity: &'a ClientIdentity, subject: &'a str) -> PolicyGrant<'a> {
    PolicyGrant {
        identity,
        subject,
        service_actions: &["workspace.create"],
        workspace_actions: &["schema.inspect", "workspace.close"],
    }
}

async fn create_handle(
    client: &mut proto::grm_service_client::GrmServiceClient<Channel>,
) -> Result<proto::WorkspaceHandle, tonic::Status> {
    Ok(client
        .create_workspace(in_memory_workspace_create_request())
        .await?
        .into_inner()
        .handle
        .unwrap())
}

async fn schema_list(
    client: &mut proto::grm_service_client::GrmServiceClient<Channel>,
    handle: &proto::WorkspaceHandle,
) -> Result<(), tonic::Status> {
    client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::SchemaList(
                    proto::SchemaListRequest {},
                )),
            }),
        })
        .await
        .map(|_| ())
}

async fn reported_policy_version(
    client: &mut proto::grm_service_client::GrmServiceClient<Channel>,
) -> String {
    client
        .security_status(proto::SecurityStatusRequest {})
        .await
        .unwrap()
        .into_inner()
        .policy_version
}

fn policy_reload_events(audit: &BoundedSecurityAuditSink) -> Vec<SecurityAuditEvent> {
    audit
        .retained_events()
        .into_iter()
        .filter(|event| event.stage == SecurityAuditStage::PolicyReload)
        .collect()
}

#[tokio::test]
async fn policy_reload_grants_added_mapping_and_permission_on_next_rpc() {
    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "reload-policy-v1",
        &[reader_grant(&fixture.client, "reader/client")],
    );
    let security = ServiceSecurityConfig::secured_from_config_file(&config).unwrap();
    let reloader = security
        .policy_reloader()
        .expect("file-backed secured config reloads");
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;
    let mut newcomer = mtls_client(&endpoint, &fixture, &fixture.rotation).await;

    let denied = create_handle(&mut newcomer).await.unwrap_err();
    assert_eq!(denied.code(), Code::Unauthenticated);
    assert_eq!(
        reported_policy_version(&mut client).await,
        "reload-policy-v1"
    );

    write_security_config(
        &config,
        "reload-policy-v2",
        &[
            reader_grant(&fixture.client, "reader/client"),
            reader_grant(&fixture.rotation, "reader/newcomer"),
        ],
    );
    let outcome = reloader.reload().unwrap();
    assert_eq!(
        outcome.previous_policy_version.as_deref(),
        Some("reload-policy-v1")
    );
    assert_eq!(outcome.policy_version.as_deref(), Some("reload-policy-v2"));

    // Same TLS channel that was refused before the reload.
    let handle = create_handle(&mut newcomer).await.unwrap();
    schema_list(&mut newcomer, &handle).await.unwrap();
    assert_eq!(
        reported_policy_version(&mut client).await,
        "reload-policy-v2"
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn policy_reload_mapping_removal_denies_next_rpc_on_established_channel_and_handle() {
    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "reload-policy-v1",
        &[
            reader_grant(&fixture.client, "reader/client"),
            reader_grant(&fixture.rotation, "reader/other"),
        ],
    );
    let security = ServiceSecurityConfig::secured_from_config_file(&config).unwrap();
    let reloader = security.policy_reloader().unwrap();
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;
    let handle = create_handle(&mut client).await.unwrap();
    schema_list(&mut client, &handle).await.unwrap();

    write_security_config(
        &config,
        "reload-policy-v2",
        &[reader_grant(&fixture.rotation, "reader/other")],
    );
    reloader.reload().unwrap();

    let execute = schema_list(&mut client, &handle).await.unwrap_err();
    assert_eq!(execute.code(), Code::Unauthenticated);
    let close = client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await
        .unwrap_err();
    assert_eq!(close.code(), Code::Unauthenticated);
    let mut other = mtls_client(&endpoint, &fixture, &fixture.rotation).await;
    create_handle(&mut other).await.unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn policy_reload_permission_removal_denies_next_rpc_on_existing_handle() {
    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "reload-policy-v1",
        &[reader_grant(&fixture.client, "reader/client")],
    );
    let security = ServiceSecurityConfig::secured_from_config_file(&config).unwrap();
    let reloader = security.policy_reloader().unwrap();
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;
    let handle = create_handle(&mut client).await.unwrap();
    schema_list(&mut client, &handle).await.unwrap();

    write_security_config(
        &config,
        "reload-policy-v2",
        &[PolicyGrant {
            identity: &fixture.client,
            subject: "reader/client",
            service_actions: &["workspace.create"],
            workspace_actions: &["workspace.close"],
        }],
    );
    reloader.reload().unwrap();

    let execute = schema_list(&mut client, &handle).await.unwrap_err();
    assert_eq!(execute.code(), Code::PermissionDenied);
    client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await
        .unwrap();

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn invalid_policy_reload_is_rejected_whole_and_previous_policy_keeps_serving() {
    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "reload-policy-v1",
        &[reader_grant(&fixture.client, "reader/client")],
    );
    let audit = BoundedSecurityAuditSink::default_local();
    let security = ServiceSecurityConfig::secured_from_config_file(&config)
        .unwrap()
        .with_audit_sink(Arc::new(audit.clone()));
    let reloader = security.policy_reloader().unwrap();
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;
    let mut newcomer = mtls_client(&endpoint, &fixture, &fixture.rotation).await;

    // A valid new mapping alongside one invalid grant: nothing may apply.
    write_security_config(
        &config,
        "reload-policy-v2",
        &[
            reader_grant(&fixture.rotation, "reader/newcomer"),
            PolicyGrant {
                identity: &fixture.client,
                subject: "reader/client",
                service_actions: &["workspace.create"],
                workspace_actions: &["workspace.obliterate"],
            },
        ],
    );
    let rejected = reloader.reload().unwrap_err();
    assert!(matches!(
        rejected,
        SecurityPolicyReloadError::InvalidConfig(_)
    ));

    fs::write(&config, b"{ not json").unwrap();
    let rejected = reloader.reload().unwrap_err();
    assert!(matches!(
        rejected,
        SecurityPolicyReloadError::InvalidConfig(_)
    ));

    fs::remove_file(&config).unwrap();
    let rejected = reloader.reload().unwrap_err();
    assert!(matches!(
        rejected,
        SecurityPolicyReloadError::InvalidConfig(_)
    ));

    let handle = create_handle(&mut client).await.unwrap();
    schema_list(&mut client, &handle).await.unwrap();
    assert_eq!(
        create_handle(&mut newcomer).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    assert_eq!(
        reported_policy_version(&mut client).await,
        "reload-policy-v1"
    );

    let events = policy_reload_events(&audit);
    assert_eq!(events.len(), 3);
    for event in &events {
        assert_eq!(event.decision, SecurityAuditDecision::Error);
        assert_eq!(event.reason, SecurityAuditReason::PolicyReloadRejected);
        assert_eq!(event.policy_version.as_deref(), Some("reload-policy-v1"));
    }

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn policy_reload_audit_records_versions_without_identity_or_path_detail() {
    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "reload-policy-v1",
        &[reader_grant(&fixture.client, "reader/client")],
    );
    let audit = BoundedSecurityAuditSink::default_local();
    let security = ServiceSecurityConfig::secured_from_config_file(&config)
        .unwrap()
        .with_audit_sink(Arc::new(audit.clone()));
    let reloader = security.policy_reloader().unwrap();

    write_security_config(
        &config,
        "reload-policy-v2",
        &[reader_grant(&fixture.rotation, "reader/newcomer")],
    );
    reloader.reload().unwrap();

    let events = policy_reload_events(&audit);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.decision, SecurityAuditDecision::Allow);
    assert_eq!(event.reason, SecurityAuditReason::PolicyReloaded);
    assert_eq!(event.policy_version.as_deref(), Some("reload-policy-v2"));
    assert!(event.authenticated_principal.is_none());
    assert!(event.operations.is_empty());
    let recorded = serde_json::to_string(event).unwrap();
    for secret in [
        fixture.client.fingerprint.as_hex(),
        fixture.rotation.fingerprint.as_hex(),
        "reader/newcomer".to_owned(),
        config.display().to_string(),
    ] {
        assert!(!recorded.contains(&secret), "audit leaked {secret}");
    }
}

/// Audit sink whose appends can be made to fail, for mandatory-audit tests.
struct FailableAuditSink {
    inner: BoundedSecurityAuditSink,
    failing: std::sync::atomic::AtomicBool,
}

impl SecurityAuditSink for FailableAuditSink {
    fn append(&self, event: SecurityAuditEvent) -> Result<(), SecurityAuditSinkError> {
        if self.failing.load(std::sync::atomic::Ordering::Acquire) {
            return Err(SecurityAuditSinkError::Unavailable);
        }
        self.inner.append(event)
    }

    fn allocate_request_id(&self) -> Result<u64, SecurityAuditSinkError> {
        self.inner.allocate_request_id()
    }

    fn allocate_event_identity(&self) -> Result<(u64, u64), SecurityAuditSinkError> {
        self.inner.allocate_event_identity()
    }
}

#[tokio::test]
async fn policy_reload_is_not_applied_when_mandatory_audit_rejects_it() {
    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "reload-policy-v1",
        &[reader_grant(&fixture.client, "reader/client")],
    );
    let audit = Arc::new(FailableAuditSink {
        inner: BoundedSecurityAuditSink::default_local(),
        failing: std::sync::atomic::AtomicBool::new(false),
    });
    let security = ServiceSecurityConfig::secured_from_config_file(&config)
        .unwrap()
        .with_audit_sink(audit.clone());
    let reloader = security.policy_reloader().unwrap();
    let (endpoint, shutdown, server) = fixture.start(security).await;
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;

    write_security_config(
        &config,
        "reload-policy-v2",
        &[reader_grant(&fixture.rotation, "reader/newcomer")],
    );
    audit
        .failing
        .store(true, std::sync::atomic::Ordering::Release);
    let rejected = reloader.reload().unwrap_err();
    assert!(matches!(
        rejected,
        SecurityPolicyReloadError::AuditUnavailable
    ));
    audit
        .failing
        .store(false, std::sync::atomic::Ordering::Release);

    create_handle(&mut client).await.unwrap();
    assert_eq!(
        reported_policy_version(&mut client).await,
        "reload-policy-v1"
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

/// Runs the real server binary and reads its stdout lines on a thread.
#[cfg(unix)]
struct ServerProcess {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

#[cfg(unix)]
impl ServerProcess {
    fn wait_for_line(&self, needle: &str) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match self.lines.recv_timeout(remaining) {
                Ok(line) if line.contains(needle) => return line,
                Ok(_) => continue,
                Err(_) => panic!("server did not print {needle:?}"),
            }
        }
    }

    fn hangup(&self) {
        let pid = libc::pid_t::try_from(self.child.id()).unwrap();
        assert_eq!(unsafe { libc::kill(pid, libc::SIGHUP) }, 0);
    }
}

#[cfg(unix)]
impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn server_binary_reloads_security_policy_on_sighup() {
    use std::io::BufRead;

    let fixture = MtlsFixture::new();
    let config = fixture.temp.path().join("security.json");
    write_security_config(
        &config,
        "sighup-policy-v1",
        &[reader_grant(&fixture.client, "reader/client")],
    );
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_grm-local-workspace-server"))
        .arg(format!("127.0.0.1:{port}"))
        .arg(fixture.temp.path().join("workspaces"))
        .env("GRM_SERVICE_SECURITY_PROFILE", "secured")
        .env("GRM_SERVICE_SECURITY_CONFIG", &config)
        .env("GRM_SERVICE_TLS_SERVER_CERT", &fixture.server_cert)
        .env("GRM_SERVICE_TLS_SERVER_KEY", &fixture.server_key)
        .env("GRM_SERVICE_TLS_CLIENT_CA_CERT", &fixture.client_ca_cert)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (sender, lines) = std::sync::mpsc::channel();
    for stream in [
        Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
        Box::new(child.stderr.take().unwrap()),
    ] {
        let sender = sender.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stream)
                .lines()
                .map_while(Result::ok)
            {
                let _ = sender.send(line);
            }
        });
    }
    let mut server = ServerProcess { child, lines };
    server.wait_for_line("security policy reload: send SIGHUP");

    let endpoint = format!("https://127.0.0.1:{port}");
    // The banner is printed before the listener binds, so wait for the port.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "server never listened"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let mut client = mtls_client(&endpoint, &fixture, &fixture.client).await;
    let mut newcomer = mtls_client(&endpoint, &fixture, &fixture.rotation).await;
    create_handle(&mut client).await.unwrap();
    assert_eq!(
        create_handle(&mut newcomer).await.unwrap_err().code(),
        Code::Unauthenticated
    );

    fs::write(&config, b"{ not json").unwrap();
    server.hangup();
    server.wait_for_line("the previous policy remains active");
    create_handle(&mut client).await.unwrap();

    write_security_config(
        &config,
        "sighup-policy-v2",
        &[
            reader_grant(&fixture.client, "reader/client"),
            reader_grant(&fixture.rotation, "reader/newcomer"),
        ],
    );
    server.hangup();
    let line = server.wait_for_line("security policy reloaded");
    assert!(
        line.contains("sighup-policy-v1 -> sighup-policy-v2"),
        "{line}"
    );
    create_handle(&mut newcomer).await.unwrap();
    assert_eq!(
        reported_policy_version(&mut client).await,
        "sighup-policy-v2"
    );

    // SIGHUP is handled, not fatal: the server has not exited.
    assert!(server.child.try_wait().unwrap().is_none());
}

#[test]
fn policy_reloader_requires_a_file_backed_secured_config() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("security.json");
    let identity = ClientIdentity {
        cert: temp.path().join("unused.crt"),
        key: temp.path().join("unused.key"),
        fingerprint: CertificateFingerprint::sha256_der(b"reader"),
    };
    write_security_config(
        &config,
        "reload-policy-v1",
        &[reader_grant(&identity, "reader")],
    );

    assert!(
        ServiceSecurityConfig::anonymous_local()
            .policy_reloader()
            .is_none()
    );
    assert!(
        ServiceSecurityConfig::docker_local_insecure()
            .policy_reloader()
            .is_none()
    );
    assert!(ServiceSecurityConfig::secured().policy_reloader().is_none());
    let file_backed = ServiceSecurityConfig::secured_from_config_file(&config).unwrap();
    assert_eq!(
        file_backed.policy_reloader().unwrap().source(),
        config.as_path()
    );
    // Replacing the policy in code detaches it from the file.
    assert!(
        file_backed
            .with_policy(Arc::new(AllowExpectedPrincipalPolicy {
                expected: test_principal(),
            }))
            .policy_reloader()
            .is_none()
    );
}

#[tokio::test]
async fn missing_or_untrusted_client_certificate_fails_mtls_boundary() {
    let fixture = MtlsFixture::new();
    let principal = test_principal();
    let authenticator =
        mapped_authenticator(vec![mapping(fixture.client.fingerprint.clone(), principal)]);
    let security = ServiceSecurityConfig::secured().with_authenticator(Arc::new(authenticator));
    let (endpoint, shutdown, server) = fixture.start(security).await;

    let missing_identity =
        GrpcClientTlsOptions::new(&fixture.server_ca_cert).with_domain_name("localhost");
    assert!(
        GrpcWorkspaceClient::connect_with_format_and_tls(
            &endpoint,
            "mtls-missing",
            GrpcWorkspaceMode::Create,
            DurabilityFormat::Binary,
            Some(missing_identity),
        )
        .await
        .is_err()
    );

    let rogue_identity = GrpcClientTlsOptions::new(&fixture.server_ca_cert)
        .with_domain_name("localhost")
        .with_identity(&fixture.rogue.cert, &fixture.rogue.key);
    assert!(
        GrpcWorkspaceClient::connect_with_format_and_tls(
            &endpoint,
            "mtls-rogue",
            GrpcWorkspaceMode::Create,
            DurabilityFormat::Binary,
            Some(rogue_identity),
        )
        .await
        .is_err()
    );

    shutdown.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn duplicate_mtls_fingerprint_mapping_configuration_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let ca = generate_local_ca_cert(
        &temp.path().join("client-ca.crt"),
        &temp.path().join("client-ca.key"),
    );
    let fingerprint = generate_client_cert(
        "client",
        &ca,
        &temp.path().join("client.crt"),
        &temp.path().join("client.key"),
    );
    let principal = test_principal();
    let error = CertificatePrincipalAuthenticator::new([
        mapping(fingerprint.clone(), principal.clone()),
        mapping(fingerprint, principal),
    ])
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "invalid certificate principal mapping configuration"
    );
}

fn generate_local_ca_cert(cert_path: &Path, key_path: &Path) -> CertifiedIssuer<'static, KeyPair> {
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "GRM Local Test CA");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
    fs::write(cert_path, issuer.pem()).unwrap();
    fs::write(key_path, issuer.key().serialize_pem()).unwrap();
    issuer
}

struct MtlsFixture {
    temp: tempfile::TempDir,
    server_ca_cert: std::path::PathBuf,
    server_cert: std::path::PathBuf,
    server_key: std::path::PathBuf,
    client_ca_cert: std::path::PathBuf,
    client: ClientIdentity,
    rotation: ClientIdentity,
    rogue: ClientIdentity,
}

impl MtlsFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let server_ca_cert = temp.path().join("server-ca.crt");
        let server_ca_key = temp.path().join("server-ca.key");
        let server_cert = temp.path().join("server.crt");
        let server_key = temp.path().join("server.key");
        let server_ca = generate_local_ca_cert(&server_ca_cert, &server_ca_key);
        generate_localhost_server_cert(&server_ca, &server_cert, &server_key);

        let client_ca_cert = temp.path().join("client-ca.crt");
        let client_ca_key = temp.path().join("client-ca.key");
        let client_ca = generate_local_ca_cert(&client_ca_cert, &client_ca_key);
        let client = ClientIdentity {
            cert: temp.path().join("client.crt"),
            key: temp.path().join("client.key"),
            fingerprint: generate_client_cert(
                "client",
                &client_ca,
                &temp.path().join("client.crt"),
                &temp.path().join("client.key"),
            ),
        };
        let rotation = ClientIdentity {
            cert: temp.path().join("client-rotation.crt"),
            key: temp.path().join("client-rotation.key"),
            fingerprint: generate_client_cert(
                "client-rotation",
                &client_ca,
                &temp.path().join("client-rotation.crt"),
                &temp.path().join("client-rotation.key"),
            ),
        };

        let rogue_ca_cert = temp.path().join("rogue-ca.crt");
        let rogue_ca_key = temp.path().join("rogue-ca.key");
        let rogue_ca = generate_local_ca_cert(&rogue_ca_cert, &rogue_ca_key);
        let rogue = ClientIdentity {
            cert: temp.path().join("rogue-client.crt"),
            key: temp.path().join("rogue-client.key"),
            fingerprint: generate_client_cert(
                "rogue-client",
                &rogue_ca,
                &temp.path().join("rogue-client.crt"),
                &temp.path().join("rogue-client.key"),
            ),
        };

        Self {
            temp,
            server_ca_cert,
            server_cert,
            server_key,
            client_ca_cert,
            client,
            rotation,
            rogue,
        }
    }

    async fn start(
        &self,
        security: ServiceSecurityConfig,
    ) -> (
        String,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = GrpcWorkspaceService::with_local_workspace_root(self.temp.path(), security)
            .into_server();
        let tls = GrpcServerTlsOptions::new(&self.server_cert, &self.server_key)
            .with_client_ca(&self.client_ca_cert)
            .server_tls_config()
            .unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .tls_config(tls)
                .unwrap()
                .add_service(service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        (
            format!("https://127.0.0.1:{port}", port = addr.port()),
            shutdown_tx,
            server,
        )
    }
}

async fn mtls_client(
    endpoint: &str,
    fixture: &MtlsFixture,
    identity: &ClientIdentity,
) -> proto::grm_service_client::GrmServiceClient<Channel> {
    let channel = grm_service_api::grpc_channel(
        endpoint,
        Some(
            &GrpcClientTlsOptions::new(&fixture.server_ca_cert)
                .with_domain_name("localhost")
                .with_identity(&identity.cert, &identity.key),
        ),
    )
    .await
    .unwrap();
    proto::grm_service_client::GrmServiceClient::new(channel)
}

fn mapped_authenticator(
    mappings: Vec<CertificatePrincipalMapping>,
) -> CertificatePrincipalAuthenticator {
    CertificatePrincipalAuthenticator::new(mappings).unwrap()
}

fn mapping(
    fingerprint: CertificateFingerprint,
    principal: Principal,
) -> CertificatePrincipalMapping {
    CertificatePrincipalMapping {
        fingerprint,
        principal,
    }
}

fn test_principal() -> Principal {
    test_principal_with_method(MTLS_CERTIFICATE_AUTHENTICATION_METHOD)
}

fn test_principal_with_method(authentication_method: &str) -> Principal {
    Principal {
        issuer: "grm-test".into(),
        subject: "service/client".into(),
        authentication_method: authentication_method.into(),
    }
}

fn in_memory_workspace_create_request() -> proto::WorkspaceCreateRequest {
    proto::WorkspaceCreateRequest {
        mode: proto::WorkspaceCreateMode::InMemory as i32,
        workspace: None,
        format: proto::DurabilityFormat::Json as i32,
    }
}

fn assert_public_error_is_redacted(
    status: &tonic::Status,
    fingerprint: &CertificateFingerprint,
    principal: &Principal,
    cert_path: &Path,
    key_path: &Path,
) {
    let error = status.to_string();
    let cert = fs::read_to_string(cert_path).unwrap();
    let key = fs::read_to_string(key_path).unwrap();
    assert!(!error.contains("BEGIN CERTIFICATE"));
    assert!(!error.contains("BEGIN PRIVATE KEY"));
    assert!(!error.contains(fingerprint.as_hex().as_str()));
    assert!(!error.contains(&principal.issuer));
    assert!(!error.contains(&principal.subject));
    assert!(!error.contains(cert.trim()));
    assert!(!error.contains(key.trim()));
}

fn generate_localhost_server_cert(
    issuer: &CertifiedIssuer<'_, KeyPair>,
    cert_path: &Path,
    key_path: &Path,
) {
    let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::from([127, 0, 0, 1])));
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key, issuer).unwrap();
    fs::write(cert_path, cert.pem()).unwrap();
    fs::write(key_path, key.serialize_pem()).unwrap();
}

fn generate_client_cert(
    name: &str,
    issuer: &CertifiedIssuer<'_, KeyPair>,
    cert_path: &Path,
    key_path: &Path,
) -> CertificateFingerprint {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let key = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key, issuer).unwrap();
    let fingerprint = CertificateFingerprint::sha256_der(cert.der().as_ref());
    fs::write(cert_path, cert.pem()).unwrap();
    fs::write(key_path, key.serialize_pem()).unwrap();
    fingerprint
}
