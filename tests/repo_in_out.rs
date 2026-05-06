mod common;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use common::{A, AB, AId, B, CountingBackend};
use grm_rs::{
    GraphBackend, GraphClient, GraphTx, InMemoryBackend, NodePattern, NodeRepository, Query,
    RelModel, RelRepository, Result,
};

/// ---- Counting backend wrapper ----
/// Counts how many times `commit()` gets called on the tx returned by `begin_tx()`.
#[tokio::test]
async fn outgoing_from_skips_wrong_labels_and_commits_after_success() -> Result<()> {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };

    // Build graph: A -> C (wrong labels for B) should be skipped
    let a_id: AId;
    {
        let mut tx = backend.begin_tx().await?;
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let c = tx
            .create_node(vec!["C".to_string()], Default::default())
            .await?;
        tx.create_relationship(a.id, c.id, AB::TYPE, Default::default())
            .await?;
        a_id = AId(a.id);
        tx.commit().await?;
        //one commit occurs here
        assert_eq!(commits.load(Ordering::SeqCst), 1);
    }

    let repo = RelRepository::<_, AB>::new(backend);

    // Now call repo.outgoing_from(&a_id) and expect empty because labels don't match B.
    let out = repo.outgoing_from(&a_id).await?;
    assert!(out.is_empty());
    // we read only - so only 1 commit is seen
    assert_eq!(commits.load(Ordering::SeqCst), 1);
    // Explanation: one commit from graph setup block, one from outgoing_from().
    Ok(())
}

#[tokio::test]
async fn outgoing_from_commits_when_relationship_decode_succeeds() -> Result<()> {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };

    // Build graph: A -> B (correct labels), but B missing required property so decode fails
    let a_id: AId;
    {
        let mut tx = backend.begin_tx().await?;
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;
        tx.create_relationship(a.id, b.id, AB::TYPE, Default::default())
            .await?;
        tx.commit().await?;
        a_id = AId(a.id);
    }

    let repo = RelRepository::<_, AB>::new(backend);

    let _ = repo
        .outgoing_from(&a_id)
        .await
        .expect("expected decode failure");

    // Ensure outgoing_from did NOT call commit on the error path.
    // commits should still be exactly 1 (from setup).
    assert_eq!(commits.load(Ordering::SeqCst), 1);

    Ok(())
}

#[tokio::test]
async fn query_return_end_does_not_commit_if_end_node_decode_fails() -> Result<()> {
    // TODO - this test calls execute - so even tho it is a read we increment the commit count
    // which means we need to differentiate in excute if this mutates or not
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };

    let a_id: AId;
    {
        let mut tx = backend.begin_tx().await?;

        // A is fine
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;

        // B has the correct label, but is missing required properties
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;

        tx.create_relationship(a.id, b.id, AB::TYPE, Default::default())
            .await?;

        tx.commit().await?;
        a_id = AId(a.id);
    }

    // Sanity: setup committed exactly once
    assert_eq!(commits.load(Ordering::SeqCst), 1);

    let repo = NodeRepository::<_, B>::new(backend);

    let q = Query::<A>::matching(
        NodePattern::<A>::new()
            .with_id(a_id)
            .out::<AB>()
            .to::<B>(),
    )
    .return_end();

    // IMPORTANT:
    // This must be a TYPED query method that decodes B before commit.
    let result = repo.execute(q).await.expect("expected decode failure");
    assert_eq!(result.gq.matches.len(), 3);

    Ok(())
}

#[tokio::test]
async fn repository_fetch_rolls_back_when_decode_fails() -> Result<()> {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };

    let a_id: AId;
    {
        let mut tx = backend.begin_tx().await?;
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;
        tx.create_relationship(a.id, b.id, AB::TYPE, Default::default())
            .await?;
        a_id = AId(a.id);
        tx.commit().await?;
    }

    assert_eq!(commits.load(Ordering::SeqCst), 1);
    assert_eq!(rollbacks.load(Ordering::SeqCst), 0);

    let repo = NodeRepository::<_, B>::new(backend);
    let q = Query::<A>::matching(NodePattern::<A>::new().with_id(a_id).out::<AB>().to::<B>())
        .return_end();

    let result = repo.fetch(q).await;

    assert!(result.is_err(), "expected B decode to fail");
    assert_eq!(
        commits.load(Ordering::SeqCst),
        1,
        "repository-managed decode failure must not commit"
    );
    assert_eq!(
        rollbacks.load(Ordering::SeqCst),
        1,
        "repository-managed decode failure should roll back its transaction"
    );

    Ok(())
}

#[tokio::test]
async fn manual_transaction_remains_caller_owned_after_decode_error() -> Result<()> {
    let commits = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend {
        inner: InMemoryBackend::new(),
        commits: commits.clone(),
        rollbacks: rollbacks.clone(),
    };
    let client = GraphClient::new(backend.clone());

    let mut tx = client.transaction().await?;
    let b = tx
        .tx_mut()?
        .create_node(vec!["B".to_string()], Default::default())
        .await?;

    let q = Query::<B>::matching(NodePattern::<B>::new());
    let exec = tx.execute(q).await?;
    let decoded = exec.decode_all::<B>();

    assert!(decoded.is_err(), "expected B decode to fail");
    assert_eq!(commits.load(Ordering::SeqCst), 0);
    assert_eq!(rollbacks.load(Ordering::SeqCst), 0);

    tx.commit().await?;

    assert_eq!(
        commits.load(Ordering::SeqCst),
        1,
        "manual transaction commits only when the caller chooses to commit"
    );
    assert_eq!(
        rollbacks.load(Ordering::SeqCst),
        0,
        "manual transaction should not auto-rollback on decode failure"
    );

    let mut read_tx = backend.begin_tx().await?;
    let found = read_tx.find_node_by_id(b.id).await?;
    read_tx.commit().await?;

    assert!(
        found.is_some(),
        "caller-owned commit should make the manually created node visible"
    );

    Ok(())
}

#[tokio::test]
async fn repo_incoming_to_returns_from_node() -> Result<()> {
    let backend = InMemoryBackend::new();

    let (a_id, b_id) = {
        let mut tx = backend.begin_tx().await?;
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;
        tx.create_relationship(a.id, b.id, AB::TYPE, Default::default())
            .await?;
        tx.commit().await?;
        (a.id, b.id)
    };

    let repo = RelRepository::<_, AB>::new(backend);

    let incoming = repo.incoming_to(&b_id.into()).await?; // convert to typed id if needed
    assert_eq!(incoming.len(), 1);

    let (_rel, from_id) = &incoming[0];
    assert_eq!(from_id.unwrap(), a_id);

    Ok(())
}

#[tokio::test]
async fn repo_incoming_to_skips_wrong_from_labels() -> Result<()> {
    use crate::common::AB;

    let backend = InMemoryBackend::new();

    let b_id: i64 = {
        let mut tx = backend.begin_tx().await?;
        let x = tx
            .create_node(vec!["X".to_string()], Default::default())
            .await?; // WRONG label (should be A)
        let b = tx
            .create_node(vec!["B".to_string()], Default::default())
            .await?;
        tx.create_relationship(x.id, b.id, AB::TYPE, Default::default())
            .await?;
        tx.commit().await?;
        b.id
    };

    let repo = RelRepository::<_, AB>::new(backend);

    let incoming = repo.incoming_to(&b_id.into()).await?;
    assert!(
        incoming.is_empty(),
        "should skip nodes whose labels don't match R::From"
    );

    Ok(())
}
