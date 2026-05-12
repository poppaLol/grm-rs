mod common;
use crate::common::*;

use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use grm_rs::{GraphClient, InMemoryBackend, NodePattern, NodeRepository, Query};

#[tokio::test]
async fn in_memory_backend_create_and_update_with_graph_client() {
    let backend = InMemoryBackend::new();
    let repo = NodeRepository::<_, User>::new(backend.clone());

    let mut user = User {
        id: UserId(0),
        name: "Alice".into(),
        age: 0,
    };
    repo.create(&mut user).await.unwrap();
    assert!(user.id.0 >= 1);

    user.name = "Bob".to_string();
    repo.update(&user).await.unwrap();

    let client = GraphClient::new(backend);
    let mut tx = client.transaction().await.unwrap();

    let q = Query::<User>::matching(NodePattern::<User>::new().filter(User::name_prop().eq("Bob")));

    let users: Vec<User> = tx.query(q).await.expect("query failed");
    tx.commit().await.unwrap();

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, user.id);
    assert_eq!(users[0].name, "Bob");
}

#[tokio::test]
async fn node_repository_delete() {
    let backend = InMemoryBackend::new();
    let repo = NodeRepository::<_, User>::new(backend);

    let mut user = User {
        id: UserId(0),
        name: "Charlie".into(),
        age: 0,
    };
    repo.create(&mut user).await.unwrap();
    let user_id = user.id;

    repo.delete(&user_id).await.unwrap();

    let fetched = repo.find_by_id(&user_id).await.unwrap();
    assert!(fetched.is_none(), "user should be deleted");
}

#[tokio::test]
async fn repository_find_by_property() {
    let backend = InMemoryBackend::new();
    let repo = NodeRepository::<_, User>::new(backend);

    let mut a = User {
        id: UserId(0),
        name: "Alice".into(),
        age: 0,
    };
    repo.create(&mut a).await.unwrap();

    let mut b = User {
        id: UserId(0),
        name: "Bob".into(),
        age: 0,
    };
    repo.create(&mut b).await.unwrap();

    let results = repo.find_by("name", &json!("Alice")).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Alice");
}

#[tokio::test]
async fn node_repository_create_many_uses_one_transaction() {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };
    let repo = NodeRepository::<_, User>::new(backend);

    let mut users = [
        User {
            id: UserId(0),
            name: "Alice".into(),
            age: 31,
        },
        User {
            id: UserId(0),
            name: "Bob".into(),
            age: 32,
        },
        User {
            id: UserId(0),
            name: "Carol".into(),
            age: 33,
        },
    ];

    repo.create_many(users.iter_mut()).await.unwrap();

    assert_eq!(commits.load(Ordering::SeqCst), 1);
    assert!(users.iter().all(|user| user.id.0 > 0));
}

#[tokio::test]
async fn node_repository_create_many_preserves_property_lookup() {
    let backend = InMemoryBackend::new();
    let repo = NodeRepository::<_, User>::new(backend);

    let mut users = [
        User {
            id: UserId(0),
            name: "Alice".into(),
            age: 31,
        },
        User {
            id: UserId(0),
            name: "Bob".into(),
            age: 32,
        },
    ];

    repo.create_many(users.iter_mut()).await.unwrap();

    let results = repo.find_by("name", &json!("Bob")).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Bob");
}

#[tokio::test]
async fn node_repository_single_creates_commit_per_insert() {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };
    let repo = NodeRepository::<_, User>::new(backend);

    let mut users = vec![
        User {
            id: UserId(0),
            name: "Alice".into(),
            age: 31,
        },
        User {
            id: UserId(0),
            name: "Bob".into(),
            age: 32,
        },
        User {
            id: UserId(0),
            name: "Carol".into(),
            age: 33,
        },
    ];

    for user in &mut users {
        repo.create(user).await.unwrap();
    }

    assert_eq!(commits.load(Ordering::SeqCst), 3);
    assert!(users.iter().all(|user| user.id.0 > 0));
}
