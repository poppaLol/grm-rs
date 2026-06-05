use std::collections::BTreeMap;

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use grm_rs::backend::{GraphStore, PersistedGraphStore};
use grm_rs::{
    CliSession, DurabilityFormat, RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeRequest,
    RuntimeValueType, SessionModelCatalog, SessionState, StoredNode, StoredRel, Workspace,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tempfile::tempdir;
use tokio::runtime::Runtime;

const ROWS: usize = 1_000;

#[derive(Clone, Serialize, Deserialize)]
struct BenchBinaryStoredNode {
    id: i64,
    labels: Vec<String>,
    props: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct BenchBinaryStoredRel {
    id: i64,
    rel_type: String,
    from: i64,
    to: i64,
    props: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct BenchBinaryPersistedGraphStore {
    next_node_id: i64,
    next_rel_id: i64,
    nodes: BTreeMap<i64, BenchBinaryStoredNode>,
    rels: BTreeMap<i64, BenchBinaryStoredRel>,
}

#[derive(Clone, Serialize, Deserialize)]
struct BenchBinaryPersistedSession {
    graph: BenchBinaryPersistedGraphStore,
    catalog: SessionModelCatalog,
}

fn bench_encode_props(props: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, Vec<u8>> {
    props
        .iter()
        .map(|(key, value)| (key.clone(), serde_json::to_vec(value).unwrap()))
        .collect()
}

fn bench_decode_props(props: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, serde_json::Value> {
    props
        .into_iter()
        .map(|(key, bytes)| (key, serde_json::from_slice(&bytes).unwrap()))
        .collect()
}

fn bench_binary_graph_projection(store: &GraphStore) -> BenchBinaryPersistedGraphStore {
    let nodes = store
        .nodes
        .iter()
        .map(|(id, node)| {
            (
                *id,
                BenchBinaryStoredNode {
                    id: node.id,
                    labels: node.labels.clone(),
                    props: bench_encode_props(&node.props),
                },
            )
        })
        .collect();

    let rels = store
        .rels
        .iter()
        .map(|(id, rel)| {
            (
                *id,
                BenchBinaryStoredRel {
                    id: rel.id,
                    rel_type: rel.rel_type.clone(),
                    from: rel.from,
                    to: rel.to,
                    props: bench_encode_props(&rel.props),
                },
            )
        })
        .collect();

    BenchBinaryPersistedGraphStore {
        next_node_id: store.next_node_id,
        next_rel_id: store.next_rel_id,
        nodes,
        rels,
    }
}

fn bench_decode_graph_without_indexes(
    graph: BenchBinaryPersistedGraphStore,
) -> PersistedGraphStore {
    let nodes = graph
        .nodes
        .into_iter()
        .map(|(id, node)| {
            (
                id,
                StoredNode {
                    id: node.id,
                    labels: node.labels,
                    props: bench_decode_props(node.props),
                },
            )
        })
        .collect();

    let rels = graph
        .rels
        .into_iter()
        .map(|(id, rel)| {
            (
                id,
                StoredRel {
                    id: rel.id,
                    rel_type: rel.rel_type,
                    from: rel.from,
                    to: rel.to,
                    props: bench_decode_props(rel.props),
                },
            )
        })
        .collect();

    PersistedGraphStore {
        next_node_id: graph.next_node_id,
        next_rel_id: graph.next_rel_id,
        nodes,
        rels,
    }
}

fn state_with_schema() -> SessionState {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new(
                "User",
                "userId",
                state.node_id_type(),
                vec![
                    RuntimeField {
                        name: "name".into(),
                        value_type: RuntimeValueType::String,
                        required: true,
                    },
                    RuntimeField {
                        name: "age".into(),
                        value_type: RuntimeValueType::Int,
                        required: true,
                    },
                ],
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_model(
            RuntimeNodeModel::new(
                "Post",
                "postId",
                state.node_id_type(),
                vec![RuntimeField {
                    name: "title".into(),
                    value_type: RuntimeValueType::String,
                    required: true,
                }],
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_rel_model(
            RuntimeRelModel::new(
                "Authored",
                "User",
                "Post",
                "authoredId",
                state.rel_id_type(),
                vec![RuntimeField {
                    name: "year".into(),
                    value_type: RuntimeValueType::Int,
                    required: true,
                }],
            )
            .unwrap(),
        )
        .unwrap();
    state
}

fn populated_state(rt: &Runtime) -> SessionState {
    let state = state_with_schema();
    rt.block_on(async {
        for index in 0..ROWS {
            let user = state
                .create_instance(
                    "User",
                    &BTreeMap::from([
                        ("name".to_string(), format!("user-{index:06}")),
                        ("age".to_string(), (18 + index % 70).to_string()),
                    ]),
                )
                .await
                .unwrap();
            let post = state
                .create_instance(
                    "Post",
                    &BTreeMap::from([("title".to_string(), format!("post-{index:06}"))]),
                )
                .await
                .unwrap();
            state
                .create_relationship_instance(
                    "Authored",
                    &user.id.to_string(),
                    &post.id.to_string(),
                    &BTreeMap::from([("year".to_string(), (2_000 + index % 25).to_string())]),
                )
                .await
                .unwrap();
        }
    });
    state
}

fn bench_save(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let state = populated_state(&rt);
    let mut group = c.benchmark_group("persistence_save_1k");

    group.bench_function("save_json", |b| {
        b.iter_batched(
            tempdir,
            |dir| {
                let dir = dir.unwrap();
                let path = dir.path().join("snapshot.json");
                state.save_to_json(&path).unwrap();
                black_box(std::fs::metadata(path).unwrap().len())
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("save_binary", |b| {
        b.iter_batched(
            tempdir,
            |dir| {
                let dir = dir.unwrap();
                let path = dir.path().join("snapshot.bin");
                state.save_to_binary(&path).unwrap();
                black_box(std::fs::metadata(path).unwrap().len())
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_load(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let state = populated_state(&rt);
    let dir = tempdir().unwrap();
    let json_path = dir.path().join("snapshot.json");
    let bin_path = dir.path().join("snapshot.bin");
    state.save_to_json(&json_path).unwrap();
    state.save_to_binary(&bin_path).unwrap();
    let mut group = c.benchmark_group("persistence_load_1k");

    group.bench_function("load_json", |b| {
        b.iter(|| {
            let mut loaded = SessionState::new();
            loaded.load_from_json(&json_path).unwrap();
            black_box(loaded)
        });
    });

    group.bench_function("load_binary", |b| {
        b.iter(|| {
            let mut loaded = SessionState::new();
            loaded.load_from_binary(&bin_path).unwrap();
            black_box(loaded)
        });
    });

    group.finish();
}

fn bench_compact(c: &mut Criterion) {
    let mut group = c.benchmark_group("autocommit_compact_1k");

    group.bench_function("compact_after_log_entries", |b| {
        b.iter_batched(
            || {
                let dir = tempdir().unwrap();
                let path = dir.path().join("session.json");
                let input = (0..ROWS)
                    .map(|index| {
                        format!(
                            "node.create User name=user-{index:06} age={}",
                            18 + index % 70
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let input = format!(
                    "model.define User userId name:string:required age:int:required\n{input}\n"
                );
                let mut session = CliSession::new(std::io::Cursor::new(input), Vec::new());
                session.enable_autocommit_json(&path).unwrap();
                (dir, path, session)
            },
            |(_dir, path, mut session)| {
                let rt = Runtime::new().unwrap();
                rt.block_on(async {
                    session.run().await.unwrap();
                });
                let summary = session.compact_autocommit().unwrap();
                black_box((summary, std::fs::metadata(path).unwrap().len()))
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_binary_workspace_persistence(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let state = populated_state(&rt);
    let workspace = Workspace::from_state(state);
    let dir = tempdir().unwrap();
    let checkpoint_path = dir.path().join("workspace.bin");
    workspace
        .checkpoint(DurabilityFormat::Binary, &checkpoint_path)
        .unwrap();

    let mut group = c.benchmark_group("persistence_binary_workspace_1k");

    group.bench_function("grm_embedded_in_memory_checkpoint_binary", |b| {
        b.iter_batched(
            tempdir,
            |dir| {
                let dir = dir.unwrap();
                let path = dir.path().join("workspace.bin");
                workspace
                    .checkpoint(DurabilityFormat::Binary, &path)
                    .unwrap();
                black_box(std::fs::metadata(path).unwrap().len())
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("grm_embedded_in_memory_reopen_binary", |b| {
        b.iter(|| {
            let reopened = Workspace::open(DurabilityFormat::Binary, &checkpoint_path).unwrap();
            black_box(reopened)
        });
    });

    group.bench_function(
        "grm_embedded_in_memory_replay_autocommit_binary_7_entries",
        |b| {
            b.iter_batched(
                || {
                    let dir = tempdir().unwrap();
                    let path = dir.path().join("workspace.bin");
                    let mut workspace = Workspace::from_state(state_with_schema());
                    workspace
                        .enable_autocommit(DurabilityFormat::Binary, &path)
                        .unwrap();
                    rt.block_on(async {
                        for index in 0..7 {
                            workspace
                                .execute_runtime(RuntimeRequest::Node(grm_rs::NodeRequest::Create(
                                    grm_rs::NodeCreateRequest {
                                        model: "User".into(),
                                        props: [
                                            (
                                                "name".into(),
                                                json!(format!("replay-user-{index:06}")),
                                            ),
                                            ("age".into(), json!(18 + index % 70)),
                                        ]
                                        .into_iter()
                                        .collect(),
                                    },
                                )))
                                .await
                                .unwrap();
                        }
                    });
                    (dir, path)
                },
                |(_dir, path)| {
                    let reopened =
                        Workspace::open_autocommit(DurabilityFormat::Binary, &path).unwrap();
                    black_box(reopened)
                },
                BatchSize::SmallInput,
            );
        },
    );

    group.finish();
}

fn bench_binary_workspace_persistence_breakdown(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let state = populated_state(&rt);
    let workspace = Workspace::from_state(state);
    let store = workspace.state().client().backend().snapshot_store();
    let catalog = workspace.state().catalog().clone();
    let binary_graph = bench_binary_graph_projection(&store);
    let binary_session = BenchBinaryPersistedSession {
        graph: binary_graph.clone(),
        catalog: catalog.clone(),
    };
    let checkpoint_bytes = bincode::serialize(&binary_session).unwrap();
    let decoded_graph = bench_decode_graph_without_indexes(binary_graph.clone());

    let dir = tempdir().unwrap();
    let checkpoint_path = dir.path().join("workspace.bin");
    workspace
        .checkpoint(DurabilityFormat::Binary, &checkpoint_path)
        .unwrap();

    let mut replay_workspace = Workspace::from_state(state_with_schema());
    let replay_path = dir.path().join("workspace-replay.bin");
    replay_workspace
        .enable_autocommit(DurabilityFormat::Binary, &replay_path)
        .unwrap();
    rt.block_on(async {
        for index in 0..7 {
            replay_workspace
                .execute_runtime(RuntimeRequest::Node(grm_rs::NodeRequest::Create(
                    grm_rs::NodeCreateRequest {
                        model: "User".into(),
                        props: [
                            ("name".into(), json!(format!("replay-user-{index:06}"))),
                            ("age".into(), json!(18 + index % 70)),
                        ]
                        .into_iter()
                        .collect(),
                    },
                )))
                .await
                .unwrap();
        }
    });

    let mut group = c.benchmark_group("persistence_binary_workspace_1k_breakdown");

    group.bench_function("checkpoint_snapshot_store_clone", |b| {
        b.iter(|| {
            let snapshot = workspace.state().client().backend().snapshot_store();
            black_box(snapshot)
        });
    });

    group.bench_function("checkpoint_catalog_clone", |b| {
        b.iter(|| {
            let catalog = workspace.state().catalog().clone();
            black_box(catalog)
        });
    });

    group.bench_function("checkpoint_binary_graph_projection_encode_props", |b| {
        b.iter(|| {
            let graph = bench_binary_graph_projection(&store);
            black_box(graph)
        });
    });

    group.bench_function("checkpoint_bincode_serialize_projected_session", |b| {
        b.iter(|| {
            let bytes = bincode::serialize(&binary_session).unwrap();
            black_box(bytes)
        });
    });

    group.bench_function("checkpoint_full_binary", |b| {
        b.iter_batched(
            tempdir,
            |dir| {
                let dir = dir.unwrap();
                let path = dir.path().join("workspace.bin");
                workspace
                    .checkpoint(DurabilityFormat::Binary, &path)
                    .unwrap();
                black_box(std::fs::metadata(path).unwrap().len())
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("reopen_fs_read_primary", |b| {
        b.iter(|| {
            let bytes = std::fs::read(&checkpoint_path).unwrap();
            black_box(bytes)
        });
    });

    group.bench_function("reopen_bincode_deserialize_session", |b| {
        b.iter(|| {
            let decoded: BenchBinaryPersistedSession =
                bincode::deserialize(&checkpoint_bytes).unwrap();
            black_box(decoded)
        });
    });

    group.bench_function("reopen_decode_props_without_indexes", |b| {
        b.iter_batched(
            || binary_graph.clone(),
            |binary_graph| {
                let graph = bench_decode_graph_without_indexes(binary_graph);
                black_box(graph)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("reopen_eager_load_index_rebuild", |b| {
        b.iter_batched(
            || decoded_graph.clone(),
            |decoded_graph| {
                let store = GraphStore::from_persisted(decoded_graph);
                black_box(store)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("reopen_load_indexes_plus_first_property_lookup", |b| {
        b.iter_batched(
            || decoded_graph.clone(),
            |decoded_graph| {
                let mut store = GraphStore::from_persisted(decoded_graph);
                let ids = store.node_ids_by_label_property("User", "name", &json!("user-000500"));
                black_box(ids)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("reopen_persisted_graph_clone", |b| {
        b.iter(|| {
            let graph = decoded_graph.clone();
            black_box(graph)
        });
    });

    group.bench_function("reopen_workspace_setup_empty_state", |b| {
        b.iter(|| {
            let workspace = Workspace::new();
            black_box(workspace)
        });
    });

    group.bench_function("reopen_full_binary", |b| {
        b.iter(|| {
            let reopened = Workspace::open(DurabilityFormat::Binary, &checkpoint_path).unwrap();
            black_box(reopened)
        });
    });

    group.bench_function("reopen_autocommit_replay_7_entries", |b| {
        b.iter(|| {
            let reopened =
                Workspace::open_autocommit(DurabilityFormat::Binary, &replay_path).unwrap();
            black_box(reopened)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_save,
    bench_load,
    bench_compact,
    bench_binary_workspace_persistence,
    bench_binary_workspace_persistence_breakdown
);
criterion_main!(benches);
