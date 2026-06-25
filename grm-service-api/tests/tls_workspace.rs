use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use grm_rs::{DefineNodeRequest, FieldSpec, FieldValueType, NodeCreateRequest};
use grm_service_api::{
    AuthorizationDecision, AuthorizationPolicy, AuthorizationReason, CertificateFingerprint,
    CertificatePrincipalAuthenticator, CertificatePrincipalMapping, DurabilityFormat,
    GrpcClientTlsOptions, GrpcServerTlsOptions, GrpcWorkspaceClient, GrpcWorkspaceMode,
    GrpcWorkspaceService, PolicyEvaluationError, Principal, SecurityRequestContext,
    ServiceSecurityConfig, proto,
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
    Principal {
        issuer: "grm-test".into(),
        subject: "service/client".into(),
        authentication_method: "mtls-certificate".into(),
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
