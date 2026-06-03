use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use grm_service_api::{GrpcWorkspaceClient, GrpcWorkspaceMode, GrpcWorkspaceService};
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

struct GrpcBenchServer {
    _root: TempDir,
    endpoint: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl GrpcBenchServer {
    fn start(rt: &Runtime) -> Self {
        let root = tempfile::tempdir().unwrap();
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let service = GrpcWorkspaceService::with_local_workspace_root(root.path()).into_server();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let task = rt.spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        Self {
            _root: root,
            endpoint: format!("http://{addr}"),
            shutdown: Some(shutdown_tx),
            task,
        }
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
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

    for rows in [250, 1_000] {
        let server = GrpcBenchServer::start(&rt);
        let data = Dataset { rows };
        let read_workspace_id = workspace_id("read", rows);
        let mut client = rt.block_on(async {
            let mut client = connect(
                server.endpoint(),
                read_workspace_id,
                GrpcWorkspaceMode::Create,
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

        let mut group = c.benchmark_group(format!("baseline_grpc_insecure_{}", size_label(rows)));
        group.sample_size(10);
        group.warm_up_time(Duration::from_secs(1));
        group.measurement_time(Duration::from_secs(3));

        group.bench_function(
            "grm_local_grpc_insecure_create_node_populated_workspace",
            |b| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iters {
                            let workspace_id = workspace_id("create", rows);
                            let mut client =
                                connect(server.endpoint(), workspace_id, GrpcWorkspaceMode::Create)
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
            "grm_local_grpc_insecure_update_node_populated_workspace",
            |b| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iters {
                            let workspace_id = workspace_id("update", rows);
                            let mut client =
                                connect(server.endpoint(), workspace_id, GrpcWorkspaceMode::Create)
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
            "grm_local_grpc_insecure_create_edge_populated_workspace",
            |b| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iters {
                            let workspace_id = workspace_id("create-edge", rows);
                            let mut client =
                                connect(server.endpoint(), workspace_id, GrpcWorkspaceMode::Create)
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
            "grm_local_grpc_insecure_update_edge_populated_workspace",
            |b| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iters {
                            let workspace_id = workspace_id("update-edge", rows);
                            let mut client =
                                connect(server.endpoint(), workspace_id, GrpcWorkspaceMode::Create)
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

        group.bench_function("grm_local_grpc_insecure_node_find_name_eq", |b| {
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

        group.bench_function("grm_local_grpc_insecure_edge_find_from", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let found = client.find_edges(edge_find_request.clone()).await.unwrap();
                    black_box(found)
                })
            });
        });

        group.bench_function("grm_local_grpc_insecure_traversal_selective", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let found = client.find_nodes(traversal_request.clone()).await.unwrap();
                    black_box(found)
                })
            });
        });

        group.bench_function("grm_local_grpc_insecure_explain_node_find", |b| {
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

        group.bench_function("grm_local_grpc_insecure_explain_edge_find", |b| {
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

        group.bench_function("grm_local_grpc_insecure_profile_edge_find", |b| {
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

        group.bench_function("grm_local_grpc_insecure_profile_node_find", |b| {
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
        server.stop(&rt);
    }
}

async fn connect(
    endpoint: &str,
    workspace_id: String,
    mode: GrpcWorkspaceMode,
) -> GrpcWorkspaceClient {
    GrpcWorkspaceClient::connect(endpoint, workspace_id, mode)
        .await
        .unwrap()
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
