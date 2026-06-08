use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

use grm_rs::{DefineNodeRequest, FieldSpec, FieldValueType, NodeCreateRequest};
use grm_service_api::{
    DurabilityFormat, GrpcClientTlsOptions, GrpcServerTlsOptions, GrpcWorkspaceClient,
    GrpcWorkspaceMode, GrpcWorkspaceService,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

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
    let service = GrpcWorkspaceService::with_local_workspace_root(temp.path()).into_server();
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
    let service = GrpcWorkspaceService::with_local_workspace_root(temp.path()).into_server();
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
    .await
    .expect("client signed by trusted client CA should connect");
    trusted.close().await.unwrap();

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
) {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let key = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key, issuer).unwrap();
    fs::write(cert_path, cert.pem()).unwrap();
    fs::write(key_path, key.serialize_pem()).unwrap();
}
