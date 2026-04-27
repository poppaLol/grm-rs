mod common;
use crate::common::*;

use serde_json::json;

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
