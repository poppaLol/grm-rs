use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use grm_service_api::{
    DurabilityFormat, GrpcClientTlsOptions, GrpcServerTlsOptions, GrpcWorkspaceClient,
    GrpcWorkspaceMode, GrpcWorkspaceService,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::runtime::Runtime;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

static WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct Dataset {
    rows: usize,
}

#[derive(Clone, Copy)]
enum TransportMode {
    Insecure,
    MutualTls,
}

impl TransportMode {
    fn group_label(self) -> &'static str {
        match self {
            Self::Insecure => "insecure",
            Self::MutualTls => "mtls",
        }
    }

    fn function_prefix(self) -> &'static str {
        match self {
            Self::Insecure => "grm_local_grpc_insecure",
            Self::MutualTls => "grm_local_grpc_mtls",
        }
    }
}

struct GrpcBenchServer {
    _root: TempDir,
    _certificates: Option<TempDir>,
    endpoint: String,
    client_tls: Option<GrpcClientTlsOptions>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl GrpcBenchServer {
    fn start(rt: &Runtime, transport: TransportMode) -> Self {
        let root = tempfile::tempdir().unwrap();
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let service = GrpcWorkspaceService::with_local_workspace_root(root.path()).into_server();
        let (certificates, server_tls, client_tls) = match transport {
            TransportMode::Insecure => (None, None, None),
            TransportMode::MutualTls => {
                let certificates = tempfile::tempdir().unwrap();
                let tls = generate_mutual_tls_material(certificates.path());
                (
                    Some(certificates),
                    Some(
                        GrpcServerTlsOptions::new(&tls.server_cert, &tls.server_key)
                            .with_client_ca(&tls.client_ca_cert)
                            .server_tls_config()
                            .unwrap(),
                    ),
                    Some(
                        GrpcClientTlsOptions::new(&tls.server_ca_cert)
                            .with_domain_name("localhost")
                            .with_identity(&tls.client_cert, &tls.client_key),
                    ),
                )
            }
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let task = rt.spawn(async move {
            let mut builder = Server::builder();
            if let Some(server_tls) = server_tls {
                builder = builder.tls_config(server_tls).unwrap();
            }
            builder
                .add_service(service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        Self {
            _root: root,
            _certificates: certificates,
            endpoint: format!(
                "{}://{addr}",
                if matches!(transport, TransportMode::MutualTls) {
                    "https"
                } else {
                    "http"
                }
            ),
            client_tls,
            shutdown: Some(shutdown_tx),
            task,
        }
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn client_tls(&self) -> Option<GrpcClientTlsOptions> {
        self.client_tls.clone()
    }

    fn stop(mut self, rt: &Runtime) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        rt.block_on(async {
            self.task.await.unwrap().unwrap();
        });
    }
}

fn bench_local_grpc_workspace(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    for transport in [TransportMode::Insecure, TransportMode::MutualTls] {
        for rows in [250, 1_000] {
            bench_transport_dataset(c, &rt, transport, rows);
        }
    }
}

fn bench_transport_dataset(c: &mut Criterion, rt: &Runtime, transport: TransportMode, rows: usize) {
    let server = GrpcBenchServer::start(rt, transport);
    let data = Dataset { rows };
    let read_workspace_id = workspace_id("read", rows);
    let mut client = rt.block_on(async {
        let mut client = connect(
            server.endpoint(),
            read_workspace_id,
            GrpcWorkspaceMode::Create,
            server.client_tls(),
        )
        .await;
        define_schema(&mut client).await;
        populate(&mut client, &data).await;
        client
    });
    let lookup_name = format!("user-{:06}", rows / 2);
    let lookup_title = format!("post-{:06}", rows / 2);
    let user_id = rows as i64 / 2 + 1;
    let post_id = user_id + rows as i64;
    let traversal_request = traversal_node_find_request(lookup_name.clone(), lookup_title);
    let edge_find_request = grm_rs::EdgeFindRequest {
        model: "Authored".into(),
        from: Some(user_id),
        ..Default::default()
    };

    let function_prefix = transport.function_prefix();
    let mut group = c.benchmark_group(format!(
        "baseline_grpc_{}_{}",
        transport.group_label(),
        size_label(rows)
    ));
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    group.bench_function(
        format!("{function_prefix}_create_node_populated_workspace"),
        |b| {
            b.iter_custom(|iters| {
                rt.block_on(async {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iters {
                        let workspace_id = workspace_id("create", rows);
                        let mut client = connect(
                            server.endpoint(),
                            workspace_id,
                            GrpcWorkspaceMode::Create,
                            server.client_tls(),
                        )
                        .await;
                        define_schema(&mut client).await;
                        populate(&mut client, &data).await;

                        let started = Instant::now();
                        let created = client
                            .create_node(grm_rs::NodeCreateRequest {
                                model: "User".into(),
                                props: [
                                    ("name".into(), json!(format!("created-{rows}"))),
                                    ("age".into(), json!(42)),
                                ]
                                .into_iter()
                                .collect(),
                            })
                            .await
                            .unwrap();
                        elapsed += started.elapsed();
                        black_box(created);
                        client.close().await.unwrap();
                    }
                    elapsed
                })
            });
        },
    );

    group.bench_function(
        format!("{function_prefix}_update_node_populated_workspace"),
        |b| {
            b.iter_custom(|iters| {
                rt.block_on(async {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iters {
                        let workspace_id = workspace_id("update", rows);
                        let mut client = connect(
                            server.endpoint(),
                            workspace_id,
                            GrpcWorkspaceMode::Create,
                            server.client_tls(),
                        )
                        .await;
                        define_schema(&mut client).await;
                        populate(&mut client, &data).await;
                        let node = client
                            .create_node(grm_rs::NodeCreateRequest {
                                model: "User".into(),
                                props: [
                                    ("name".into(), json!("updatable")),
                                    ("age".into(), json!(40)),
                                ]
                                .into_iter()
                                .collect(),
                            })
                            .await
                            .unwrap();

                        let started = Instant::now();
                        let updated = client
                            .update_node(grm_rs::NodeUpdateRequest {
                                model: "User".into(),
                                id: node.id,
                                props: [("age".into(), json!(41))].into_iter().collect(),
                            })
                            .await
                            .unwrap();
                        elapsed += started.elapsed();
                        black_box(updated);
                        client.close().await.unwrap();
                    }
                    elapsed
                })
            });
        },
    );

    group.bench_function(
        format!("{function_prefix}_create_edge_populated_workspace"),
        |b| {
            b.iter_custom(|iters| {
                rt.block_on(async {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iters {
                        let workspace_id = workspace_id("create-edge", rows);
                        let mut client = connect(
                            server.endpoint(),
                            workspace_id,
                            GrpcWorkspaceMode::Create,
                            server.client_tls(),
                        )
                        .await;
                        define_schema(&mut client).await;
                        populate(&mut client, &data).await;

                        let started = Instant::now();
                        let created = client
                            .create_edge(grm_rs::EdgeCreateRequest {
                                model: "Authored".into(),
                                from: user_id,
                                to: post_id,
                                props: [("year".into(), json!(2026))].into_iter().collect(),
                            })
                            .await
                            .unwrap();
                        elapsed += started.elapsed();
                        black_box(created);
                        client.close().await.unwrap();
                    }
                    elapsed
                })
            });
        },
    );

    group.bench_function(
        format!("{function_prefix}_update_edge_populated_workspace"),
        |b| {
            b.iter_custom(|iters| {
                rt.block_on(async {
                    let mut elapsed = Duration::ZERO;
                    for _ in 0..iters {
                        let workspace_id = workspace_id("update-edge", rows);
                        let mut client = connect(
                            server.endpoint(),
                            workspace_id,
                            GrpcWorkspaceMode::Create,
                            server.client_tls(),
                        )
                        .await;
                        define_schema(&mut client).await;
                        populate(&mut client, &data).await;

                        let started = Instant::now();
                        let updated = client
                            .update_edge(grm_rs::EdgeUpdateRequest {
                                model: "Authored".into(),
                                id: user_id,
                                props: [("year".into(), json!(2027))].into_iter().collect(),
                            })
                            .await
                            .unwrap();
                        elapsed += started.elapsed();
                        black_box(updated);
                        client.close().await.unwrap();
                    }
                    elapsed
                })
            });
        },
    );

    group.bench_function(format!("{function_prefix}_node_find_name_eq"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let found = client
                    .find_nodes(grm_rs::NodeFindRequest {
                        model: "User".into(),
                        predicates: vec![predicate("name", json!(lookup_name.clone()))],
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                black_box(found)
            })
        });
    });

    group.bench_function(format!("{function_prefix}_edge_find_from"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let found = client.find_edges(edge_find_request.clone()).await.unwrap();
                black_box(found)
            })
        });
    });

    group.bench_function(format!("{function_prefix}_traversal_selective"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let found = client.find_nodes(traversal_request.clone()).await.unwrap();
                black_box(found)
            })
        });
    });

    group.bench_function(format!("{function_prefix}_explain_node_find"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let explained = client
                    .explain(grm_rs::ExplainRequest {
                        query: grm_rs::QueryRequest::NodeFind(traversal_request.clone()),
                    })
                    .await
                    .unwrap();
                black_box(explained)
            })
        });
    });

    group.bench_function(format!("{function_prefix}_explain_edge_find"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let explained = client
                    .explain(grm_rs::ExplainRequest {
                        query: grm_rs::QueryRequest::EdgeFind(edge_find_request.clone()),
                    })
                    .await
                    .unwrap();
                black_box(explained)
            })
        });
    });

    group.bench_function(format!("{function_prefix}_profile_edge_find"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let profiled = client
                    .profile(grm_rs::ProfileRequest {
                        query: grm_rs::QueryRequest::EdgeFind(edge_find_request.clone()),
                    })
                    .await
                    .unwrap();
                black_box(profiled)
            })
        });
    });

    group.bench_function(format!("{function_prefix}_profile_node_find"), |b| {
        b.iter(|| {
            rt.block_on(async {
                let profiled = client
                    .profile(grm_rs::ProfileRequest {
                        query: grm_rs::QueryRequest::NodeFind(traversal_request.clone()),
                    })
                    .await
                    .unwrap();
                black_box(profiled)
            })
        });
    });

    group.finish();
    rt.block_on(async {
        client.close().await.unwrap();
    });
    server.stop(rt);
}

async fn connect(
    endpoint: &str,
    workspace_id: String,
    mode: GrpcWorkspaceMode,
    tls: Option<GrpcClientTlsOptions>,
) -> GrpcWorkspaceClient {
    GrpcWorkspaceClient::connect_with_format_and_tls(
        endpoint,
        workspace_id,
        mode,
        DurabilityFormat::Binary,
        tls,
    )
    .await
    .unwrap()
}

struct MutualTlsMaterial {
    server_ca_cert: std::path::PathBuf,
    server_cert: std::path::PathBuf,
    server_key: std::path::PathBuf,
    client_ca_cert: std::path::PathBuf,
    client_cert: std::path::PathBuf,
    client_key: std::path::PathBuf,
}

fn generate_mutual_tls_material(root: &Path) -> MutualTlsMaterial {
    let server_ca_cert = root.join("server-ca.crt");
    let server_ca_key = root.join("server-ca.key");
    let server_cert = root.join("server.crt");
    let server_key = root.join("server.key");
    let server_ca = generate_local_ca_cert(&server_ca_cert, &server_ca_key);
    generate_localhost_server_cert(&server_ca, &server_cert, &server_key);

    let client_ca_cert = root.join("client-ca.crt");
    let client_ca_key = root.join("client-ca.key");
    let client_cert = root.join("client.crt");
    let client_key = root.join("client.key");
    let client_ca = generate_local_ca_cert(&client_ca_cert, &client_ca_key);
    generate_client_cert("grm-local-benchmark", &client_ca, &client_cert, &client_key);

    MutualTlsMaterial {
        server_ca_cert,
        server_cert,
        server_key,
        client_ca_cert,
        client_cert,
        client_key,
    }
}

fn generate_local_ca_cert(cert_path: &Path, key_path: &Path) -> CertifiedIssuer<'static, KeyPair> {
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "GRM Local Benchmark CA");
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

async fn define_schema(client: &mut GrpcWorkspaceClient) {
    client
        .define_node(grm_rs::DefineNodeRequest {
            name: "User".into(),
            id_field: "userId".into(),
            fields: vec![
                grm_rs::FieldSpec {
                    name: "name".into(),
                    value_type: grm_rs::FieldValueType::String,
                    required: true,
                },
                grm_rs::FieldSpec {
                    name: "age".into(),
                    value_type: grm_rs::FieldValueType::Int,
                    required: true,
                },
            ],
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
}

async fn populate(client: &mut GrpcWorkspaceClient, data: &Dataset) {
    let mut users = Vec::with_capacity(data.rows);
    let mut posts = Vec::with_capacity(data.rows);

    for index in 0..data.rows {
        users.push(
            client
                .create_node(grm_rs::NodeCreateRequest {
                    model: "User".into(),
                    props: [
                        ("name".into(), json!(format!("user-{index:06}"))),
                        ("age".into(), json!(18 + index % 70)),
                    ]
                    .into_iter()
                    .collect(),
                })
                .await
                .unwrap()
                .id,
        );
    }
    for index in 0..data.rows {
        posts.push(
            client
                .create_node(grm_rs::NodeCreateRequest {
                    model: "Post".into(),
                    props: [("title".into(), json!(format!("post-{index:06}")))]
                        .into_iter()
                        .collect(),
                })
                .await
                .unwrap()
                .id,
        );
    }
    for index in 0..data.rows {
        client
            .create_edge(grm_rs::EdgeCreateRequest {
                model: "Authored".into(),
                from: users[index],
                to: posts[index],
                props: [("year".into(), json!(2_000 + index % 25))]
                    .into_iter()
                    .collect(),
            })
            .await
            .unwrap();
    }
}

fn traversal_node_find_request(name: String, title: String) -> grm_rs::NodeFindRequest {
    grm_rs::NodeFindRequest {
        model: "User".into(),
        predicates: vec![predicate("name", json!(name))],
        traversals: vec![grm_rs::TraversalStepRequest {
            direction: grm_rs::TraversalDirection::Out,
            edge_model: Some("Authored".into()),
            end_model: "Post".into(),
        }],
        end_predicates: vec![predicate("title", json!(title))],
        return_mode: Some(grm_rs::TraversalReturn::End),
        ..Default::default()
    }
}

fn predicate(field: &str, value: Value) -> grm_rs::PropertyPredicate {
    grm_rs::PropertyPredicate {
        field: field.into(),
        op: grm_rs::PredicateOp::Eq,
        value,
    }
}

fn workspace_id(prefix: &str, rows: usize) -> String {
    let counter = WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("bench-{prefix}-{rows}-{counter}")
}

fn size_label(rows: usize) -> String {
    if rows >= 1_000 {
        format!("{}k", rows / 1_000)
    } else {
        rows.to_string()
    }
}

criterion_group!(benches, bench_local_grpc_workspace);
criterion_main!(benches);
