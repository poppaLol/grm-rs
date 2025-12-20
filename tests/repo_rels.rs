mod common;
use crate::common::*;

use grm_rs::{InMemoryBackend, NodeRepository, RelRepository};

#[tokio::test]
async fn rel_repository_create_and_outgoing() {
    let backend = InMemoryBackend::new();

    let user_repo = NodeRepository::<_, User>::new(backend.clone());
    let post_repo = NodeRepository::<_, Post>::new(backend.clone());
    let rel_repo = RelRepository::<_, Authored>::new(backend.clone());

    let mut user = User {
        id: UserId(0),
        name: "Alice".into(),
        age: 0,
    };
    user_repo.create(&mut user).await.unwrap();

    let mut post = Post {
        id: PostId(0),
        title: "Hello Graph".into(),
    };
    post_repo.create(&mut post).await.unwrap();

    let mut authored = Authored {
        id: AuthoredId(0),
        year: 2024,
    };
    rel_repo
        .create_between(&user.id, &post.id, &mut authored)
        .await
        .unwrap();
    assert!(i64::from(authored.id) >= 1);

    let edges = rel_repo.outgoing_from(&user.id).await.unwrap();

    let (rel, target_post) = &edges[0];
    assert_eq!(rel.year, 2024);
    assert_eq!(target_post.title, "Hello Graph");
}

#[tokio::test]
async fn deleting_node_removes_outgoing_relationships() {
    let backend = InMemoryBackend::new();

    let user_repo = NodeRepository::<_, User>::new(backend.clone());
    let post_repo = NodeRepository::<_, Post>::new(backend.clone());
    let rel_repo = RelRepository::<_, Authored>::new(backend.clone());

    let mut user = User {
        id: UserId(0),
        name: "Alice".into(),
        age: 0,
    };
    user_repo.create(&mut user).await.unwrap();

    let mut post = Post {
        id: PostId(0),
        title: "Hello Graph".into(),
    };
    post_repo.create(&mut post).await.unwrap();

    let mut authored = Authored {
        id: AuthoredId(0),
        year: 2024,
    };
    rel_repo
        .create_between(&user.id, &post.id, &mut authored)
        .await
        .unwrap();

    let edges_before = rel_repo.outgoing_from(&user.id).await.unwrap();
    assert_eq!(edges_before.len(), 1);

    user_repo.delete(&user.id).await.unwrap();

    let edges_after = rel_repo.outgoing_from(&user.id).await.unwrap();
    assert_eq!(edges_after.len(), 0);
}
