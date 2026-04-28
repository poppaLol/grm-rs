use std::collections::BTreeMap;

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use grm_rs::{
    CliSession, RuntimeField, RuntimeNodeModel, RuntimeRelModel, RuntimeValueType, SessionState,
};
use tempfile::tempdir;
use tokio::runtime::Runtime;

const ROWS: usize = 1_000;

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

criterion_group!(benches, bench_save, bench_load, bench_compact);
criterion_main!(benches);
