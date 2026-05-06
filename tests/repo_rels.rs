mod common;
use crate::common::*;

use grm_rs::{InMemoryBackend, NodeRepository, RelRepository};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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
        from: UserId::default(),
        to: PostId::default(),
    };
    rel_repo
        .create_between(&user.id, &post.id, &mut authored)
        .await
        .unwrap();
    assert!(i64::from(authored.id) >= 1);

    let edges = rel_repo.outgoing_from(&user.id).await.unwrap();

    let (rel, target_id) = &edges[0];
    assert_eq!(rel.year, 2024);
    assert_eq!(target_id.unwrap(), i64::from(post.id));
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
        from: UserId::default(),
        to: PostId::default(),
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

#[tokio::test]
async fn rel_repository_create_many_between_uses_one_transaction() {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };

    let user_repo = NodeRepository::<_, User>::new(backend.clone());
    let post_repo = NodeRepository::<_, Post>::new(backend.clone());
    let rel_repo = RelRepository::<_, Authored>::new(backend);

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
    user_repo.create_many(users.iter_mut()).await.unwrap();

    let mut posts = [
        Post {
            id: PostId(0),
            title: "One".into(),
        },
        Post {
            id: PostId(0),
            title: "Two".into(),
        },
        Post {
            id: PostId(0),
            title: "Three".into(),
        },
    ];
    post_repo.create_many(posts.iter_mut()).await.unwrap();

    commits.store(0, Ordering::SeqCst);

    let mut authored = [
        (
            users[0].id,
            posts[0].id,
            Authored {
                id: AuthoredId(0),
                year: 2024,
                from: UserId::default(),
                to: PostId::default(),
            },
        ),
        (
            users[1].id,
            posts[1].id,
            Authored {
                id: AuthoredId(0),
                year: 2025,
                from: UserId::default(),
                to: PostId::default(),
            },
        ),
        (
            users[2].id,
            posts[2].id,
            Authored {
                id: AuthoredId(0),
                year: 2026,
                from: UserId::default(),
                to: PostId::default(),
            },
        ),
    ];

    rel_repo
        .create_many_between(
            authored
                .iter_mut()
                .map(|(from_id, to_id, rel)| (&*from_id, &*to_id, rel)),
        )
        .await
        .unwrap();

    assert_eq!(commits.load(Ordering::SeqCst), 1);
    assert!(authored.iter().all(|(_, _, rel)| i64::from(rel.id) > 0));
}
