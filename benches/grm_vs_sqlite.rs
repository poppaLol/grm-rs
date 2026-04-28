use std::collections::BTreeMap;
use std::time::Duration;

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use grm_rs::{
    GraphTx, InMemoryBackend, NodeModel, NodeRepository, RelModel, RelRepository, RuntimeField,
    RuntimeNodeModel, RuntimeRelModel, RuntimeValueType, SessionState, typed_id,
};
use rusqlite::{Connection, params};
use serde_json::json;
use tokio::runtime::Runtime;

const MAX_ROWS_FOR_SINGLE_INSERT_BENCHES: usize = 1_000;

typed_id!(BenchUserId);
typed_id!(BenchPostId);
typed_id!(BenchAuthoredId);

#[derive(Clone, NodeModel)]
struct BenchUser {
    #[grm(id)]
    id: BenchUserId,
    name: String,
    age: i32,
}

#[derive(Clone, NodeModel)]
struct BenchPost {
    #[grm(id)]
    id: BenchPostId,
    title: String,
}

#[derive(Clone, RelModel)]
#[grm(from = "BenchUser", to = "BenchPost", ty = "AUTHORED")]
struct BenchAuthored {
    #[grm(id)]
    id: BenchAuthoredId,
    year: u64,
    #[grm(skip)]
    #[allow(dead_code)]
    from: BenchUserId,
    #[grm(skip)]
    #[allow(dead_code)]
    to: BenchPostId,
}

#[derive(Clone)]
struct UserInput {
    name: String,
    age: i64,
}

#[derive(Clone)]
struct PostInput {
    title: String,
}

#[derive(Clone)]
struct AuthoredInput {
    user_id: i64,
    post_id: i64,
    year: i64,
}

#[derive(Clone)]
struct Dataset {
    users: Vec<UserInput>,
    posts: Vec<PostInput>,
    authored: Vec<AuthoredInput>,
}

fn dataset(size: usize) -> Dataset {
    let users = (0..size)
        .map(|index| UserInput {
            name: format!("user-{index:06}"),
            age: 18 + (index % 70) as i64,
        })
        .collect::<Vec<_>>();
    let posts = (0..size)
        .map(|index| PostInput {
            title: format!("post-{index:06}"),
        })
        .collect::<Vec<_>>();
    let authored = (0..size)
        .map(|index| AuthoredInput {
            user_id: index as i64 + 1,
            post_id: index as i64 + 1,
            year: 2_000 + (index % 25) as i64,
        })
        .collect::<Vec<_>>();

    Dataset {
        users,
        posts,
        authored,
    }
}

fn grm_state_with_schema() -> SessionState {
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

fn populate_grm(rt: &Runtime, dataset: &Dataset) -> SessionState {
    let state = grm_state_with_schema();
    rt.block_on(async {
        for user in &dataset.users {
            state
                .create_instance(
                    "User",
                    &BTreeMap::from([
                        ("name".to_string(), user.name.clone()),
                        ("age".to_string(), user.age.to_string()),
                    ]),
                )
                .await
                .unwrap();
        }
        for post in &dataset.posts {
            state
                .create_instance(
                    "Post",
                    &BTreeMap::from([("title".to_string(), post.title.clone())]),
                )
                .await
                .unwrap();
        }
        for authored in &dataset.authored {
            let grm_post_id = authored.post_id + dataset.users.len() as i64;
            state
                .create_relationship_instance(
                    "Authored",
                    &authored.user_id.to_string(),
                    &grm_post_id.to_string(),
                    &BTreeMap::from([("year".to_string(), authored.year.to_string())]),
                )
                .await
                .unwrap();
        }
    });
    state
}

fn populate_grm_bulk(rt: &Runtime, dataset: &Dataset) -> SessionState {
    let state = grm_state_with_schema();
    rt.block_on(async {
        let mut tx = state.client().transaction().await.unwrap();
        for user in &dataset.users {
            tx.tx_mut()
                .unwrap()
                .create_node(
                    vec!["User".to_string()],
                    BTreeMap::from([
                        ("name".to_string(), json!(user.name.clone())),
                        ("age".to_string(), json!(user.age)),
                    ]),
                )
                .await
                .unwrap();
        }
        for post in &dataset.posts {
            tx.tx_mut()
                .unwrap()
                .create_node(
                    vec!["Post".to_string()],
                    BTreeMap::from([("title".to_string(), json!(post.title.clone()))]),
                )
                .await
                .unwrap();
        }
        for authored in &dataset.authored {
            let grm_post_id = authored.post_id + dataset.users.len() as i64;
            tx.tx_mut()
                .unwrap()
                .create_relationship(
                    authored.user_id,
                    grm_post_id,
                    "Authored",
                    BTreeMap::from([("year".to_string(), json!(authored.year))]),
                )
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    });
    state
}

fn populate_grm_repo_single(rt: &Runtime, dataset: &Dataset) -> InMemoryBackend {
    let backend = InMemoryBackend::new();
    let user_repo = NodeRepository::<_, BenchUser>::new(backend.clone());
    let post_repo = NodeRepository::<_, BenchPost>::new(backend.clone());
    let rel_repo = RelRepository::<_, BenchAuthored>::new(backend.clone());

    rt.block_on(async {
        let mut users = dataset
            .users
            .iter()
            .map(|user| BenchUser {
                id: BenchUserId(0),
                name: user.name.clone(),
                age: user.age as i32,
            })
            .collect::<Vec<_>>();
        for user in &mut users {
            user_repo.create(user).await.unwrap();
        }

        let mut posts = dataset
            .posts
            .iter()
            .map(|post| BenchPost {
                id: BenchPostId(0),
                title: post.title.clone(),
            })
            .collect::<Vec<_>>();
        for post in &mut posts {
            post_repo.create(post).await.unwrap();
        }

        for authored in &dataset.authored {
            let mut rel = BenchAuthored {
                id: BenchAuthoredId(0),
                year: authored.year as u64,
                from: BenchUserId::default(),
                to: BenchPostId::default(),
            };
            rel_repo
                .create_between(
                    &users[authored.user_id as usize - 1].id,
                    &posts[authored.post_id as usize - 1].id,
                    &mut rel,
                )
                .await
                .unwrap();
        }
    });

    backend
}

fn populate_grm_repo_bulk(rt: &Runtime, dataset: &Dataset) -> InMemoryBackend {
    let backend = InMemoryBackend::new();
    let user_repo = NodeRepository::<_, BenchUser>::new(backend.clone());
    let post_repo = NodeRepository::<_, BenchPost>::new(backend.clone());
    let rel_repo = RelRepository::<_, BenchAuthored>::new(backend.clone());

    rt.block_on(async {
        let mut users = dataset
            .users
            .iter()
            .map(|user| BenchUser {
                id: BenchUserId(0),
                name: user.name.clone(),
                age: user.age as i32,
            })
            .collect::<Vec<_>>();
        user_repo.create_many(users.iter_mut()).await.unwrap();

        let mut posts = dataset
            .posts
            .iter()
            .map(|post| BenchPost {
                id: BenchPostId(0),
                title: post.title.clone(),
            })
            .collect::<Vec<_>>();
        post_repo.create_many(posts.iter_mut()).await.unwrap();

        let mut authored = dataset
            .authored
            .iter()
            .map(|rel| {
                (
                    users[rel.user_id as usize - 1].id,
                    posts[rel.post_id as usize - 1].id,
                    BenchAuthored {
                        id: BenchAuthoredId(0),
                        year: rel.year as u64,
                        from: BenchUserId::default(),
                        to: BenchPostId::default(),
                    },
                )
            })
            .collect::<Vec<_>>();

        rel_repo
            .create_many_between(
                authored
                    .iter_mut()
                    .map(|(from_id, to_id, rel)| (&*from_id, &*to_id, rel)),
            )
            .await
            .unwrap();
    });

    backend
}

fn sqlite_connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            age INTEGER NOT NULL
        );
        CREATE TABLE posts (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL
        );
        CREATE TABLE authored (
            id INTEGER PRIMARY KEY,
            from_user INTEGER NOT NULL,
            to_post INTEGER NOT NULL,
            year INTEGER NOT NULL
        );
        CREATE INDEX idx_users_name ON users(name);
        CREATE INDEX idx_users_age ON users(age);
        CREATE INDEX idx_authored_from ON authored(from_user);
        ",
    )
    .unwrap();
    conn
}

fn populate_sqlite(dataset: &Dataset) -> Connection {
    let mut conn = sqlite_connection();
    let tx = conn.transaction().unwrap();
    {
        let mut insert_user = tx
            .prepare("INSERT INTO users (name, age) VALUES (?1, ?2)")
            .unwrap();
        for user in &dataset.users {
            insert_user.execute(params![user.name, user.age]).unwrap();
        }
    }
    {
        let mut insert_post = tx.prepare("INSERT INTO posts (title) VALUES (?1)").unwrap();
        for post in &dataset.posts {
            insert_post.execute(params![post.title]).unwrap();
        }
    }
    {
        let mut insert_authored = tx
            .prepare("INSERT INTO authored (from_user, to_post, year) VALUES (?1, ?2, ?3)")
            .unwrap();
        for authored in &dataset.authored {
            insert_authored
                .execute(params![authored.user_id, authored.post_id, authored.year])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    conn
}

fn bench_inserts(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let profile_grm_insert_only = std::env::var_os("GRM_BENCH_PROFILE_GRM_INSERT_ONLY").is_some();
    for rows in insert_rows() {
        let data = dataset(rows);
        let group_name = format!("insert_{}", size_label(rows));
        let mut group = c.benchmark_group(group_name);
        group.sample_size(10);
        group.warm_up_time(Duration::from_secs(1));
        group.measurement_time(Duration::from_secs(3));

        if !profile_grm_insert_only && rows <= MAX_ROWS_FOR_SINGLE_INSERT_BENCHES {
            group.bench_function("grm_session_state", |b| {
                b.iter_batched(
                    || data.clone(),
                    |data| black_box(populate_grm(&rt, &data)),
                    BatchSize::SmallInput,
                );
            });

            group.bench_function("grm_repo_single_transactions", |b| {
                b.iter_batched(
                    || data.clone(),
                    |data| black_box(populate_grm_repo_single(&rt, &data)),
                    BatchSize::SmallInput,
                );
            });
        }

        group.bench_function("grm_repo_bulk_transactions", |b| {
            b.iter_batched(
                || data.clone(),
                |data| black_box(populate_grm_repo_bulk(&rt, &data)),
                BatchSize::SmallInput,
            );
        });

        if !profile_grm_insert_only {
            group.bench_function("sqlite_in_memory_transaction", |b| {
                b.iter_batched(
                    || data.clone(),
                    |data| black_box(populate_sqlite(&data)),
                    BatchSize::SmallInput,
                );
            });
        }

        group.finish();
    }
}

fn bench_property_lookup(c: &mut Criterion) {
    if std::env::var_os("GRM_BENCH_PROFILE_GRM_INSERT_ONLY").is_some() {
        return;
    }

    let rt = Runtime::new().unwrap();
    for rows in read_rows() {
        let data = dataset(rows);
        let grm = populate_grm_bulk(&rt, &data);
        let sqlite = populate_sqlite(&data);
        let lookup_name = format!("user-{:06}", rows / 2);
        let group_name = format!("property_lookup_{}", size_label(rows));
        let mut group = c.benchmark_group(group_name);

        group.bench_function("grm_index_name_eq", |b| {
            b.iter(|| {
                let rows = grm
                    .find_nodes(
                        "User",
                        &BTreeMap::from([("name".to_string(), lookup_name.clone())]),
                    )
                    .unwrap();
                black_box(rows)
            });
        });

        group.bench_function("sqlite_index_name_eq", |b| {
            b.iter(|| {
                let mut stmt = sqlite
                    .prepare("SELECT id, name, age FROM users WHERE name = ?1")
                    .unwrap();
                let rows = stmt
                    .query_map(params![lookup_name], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                black_box(rows)
            });
        });

        group.finish();
    }
}

fn bench_one_hop(c: &mut Criterion) {
    if std::env::var_os("GRM_BENCH_PROFILE_GRM_INSERT_ONLY").is_some() {
        return;
    }

    let rt = Runtime::new().unwrap();
    for rows in read_rows() {
        let data = dataset(rows);
        let grm = populate_grm_bulk(&rt, &data);
        let sqlite = populate_sqlite(&data);
        let user_id = (rows / 2 + 1).to_string();
        let sqlite_user_id = rows as i64 / 2 + 1;
        let group_name = format!("one_hop_authored_{}", size_label(rows));
        let mut group = c.benchmark_group(group_name);

        group.bench_function("grm_edge_lookup_from_user", |b| {
            b.iter(|| {
                let rows = grm
                    .find_relationships(
                        "Authored",
                        &BTreeMap::from([("from".to_string(), user_id.clone())]),
                    )
                    .unwrap();
                black_box(rows)
            });
        });

        group.bench_function("sqlite_edge_lookup_from_user", |b| {
            b.iter(|| {
                let mut stmt = sqlite
                    .prepare(
                        "SELECT id, from_user, to_post, year FROM authored WHERE from_user = ?1",
                    )
                    .unwrap();
                let rows = stmt
                    .query_map(params![sqlite_user_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                black_box(rows)
            });
        });

        group.bench_function("grm_edge_lookup_then_post_lookup", |b| {
            b.iter(|| {
                let posts = grm
                    .find_relationships(
                        "Authored",
                        &BTreeMap::from([("from".to_string(), user_id.clone())]),
                    )
                    .unwrap()
                    .into_iter()
                    .map(|rel| {
                        grm.find_nodes(
                            "Post",
                            &BTreeMap::from([("id".to_string(), rel.to.to_string())]),
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                black_box(posts)
            });
        });

        group.bench_function("sqlite_index_join_from_user", |b| {
            b.iter(|| {
                let mut stmt = sqlite
                    .prepare(
                        "
                    SELECT posts.id, posts.title
                    FROM authored
                    JOIN posts ON posts.id = authored.to_post
                    WHERE authored.from_user = ?1
                    ",
                    )
                    .unwrap();
                let rows = stmt
                    .query_map(params![sqlite_user_id], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                black_box(rows)
            });
        });

        group.finish();
    }
}

fn size_label(rows: usize) -> String {
    if rows >= 1_000 {
        format!("{}k", rows / 1_000)
    } else {
        rows.to_string()
    }
}

fn read_rows() -> Vec<usize> {
    let mut rows = vec![1_000, 10_000];
    if std::env::var_os("GRM_BENCH_STRESS").is_some() {
        rows.push(100_000);
    }
    rows
}

fn insert_rows() -> Vec<usize> {
    let mut rows = vec![250, 1_000];
    if std::env::var_os("GRM_BENCH_STRESS").is_some() {
        rows.push(10_000);
    }
    rows
}

criterion_group!(benches, bench_inserts, bench_property_lookup, bench_one_hop);
criterion_main!(benches);
