mod common;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use common::{AB, CountingBackend};
use grm_rs::{GraphBackend, GraphTx, InMemoryBackend, NodeModel, RelModel, RelRepository, Result};


/// ---- Counting backend wrapper ----
/// Counts how many times `commit()` gets called on the tx returned by `begin_tx()`.
#[tokio::test]
async fn outgoing_from_skips_wrong_labels_and_commits_after_success() -> Result<()> {
    let commits = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend { inner: InMemoryBackend::new(), commits: commits.clone() };

    // Build graph: A -> C (wrong labels for B) should be skipped
    let a_id: i64;
    {
        let mut tx = backend.begin_tx().await?;
        let a = tx.create_node(vec!["A".to_string()], Default::default()).await?;
        let c = tx.create_node(vec!["C".to_string()], Default::default()).await?;
        tx.create_relationship(a.id, c.id, AB::TYPE.to_string(), Default::default()).await?;
        a_id = a.id;
        tx.commit().await?;
    }

    let repo = RelRepository::<_, AB>::new(backend);

    // Now call repo.outgoing_from(&a_id) and expect empty because labels don't match B.
    let out = repo.outgoing_from(&a_id).await?;
    assert!(out.is_empty());
    // outgoing_from should commit once on success
    assert_eq!(commits.load(Ordering::SeqCst), 2); 
    // Explanation: one commit from graph setup block, one from outgoing_from().
    Ok(())
}

#[tokio::test]
async fn outgoing_from_does_not_commit_if_decode_fails() -> Result<()> {
    let commits = Arc::new(AtomicUsize::new(0));
    let backend = CountingBackend { inner: InMemoryBackend::new(), commits: commits.clone() };

    // Build graph: A -> B (correct labels), but B missing required property so decode fails
    let a_id: i64;
    {
        let mut tx = backend.begin_tx().await?;
        let a = tx.create_node(vec!["A".to_string()], Default::default()).await?;
        let b = tx.create_node(vec!["B".to_string()], Default::default()).await?;
        tx.create_relationship(a.id, b.id, AB::TYPE.to_string(), Default::default()).await?;
        tx.commit().await?;
        a_id = a.id;
    }

    let repo = RelRepository::<_, AB>::new(backend);

    let err = repo.outgoing_from(&a_id).await.err().expect("expected decode failure");

    // Ensure outgoing_from did NOT call commit on the error path.
    // commits should still be exactly 1 (from setup).
    assert_eq!(commits.load(Ordering::SeqCst), 1);

    // Optionally assert error kind/message if you want:
    let _ = err;
    Ok(())
}

#[tokio::test]
async fn repo_incoming_to_returns_from_node() -> Result<()> {
    let backend = InMemoryBackend::new();

    let (a_id, b_id) = {
        let mut tx = backend.begin_tx().await?;
        let a = tx.create_node(vec!["A".to_string()], Default::default()).await?;
        let b = tx.create_node(vec!["B".to_string()], Default::default()).await?;
        tx.create_relationship(a.id, b.id, AB::TYPE.to_string(), Default::default()).await?;
        tx.commit().await?;
        (a.id, b.id)
    };

    let repo = RelRepository::<_, AB>::new(backend);

    let incoming = repo.incoming_to(&b_id.into()).await?; // convert to typed id if needed
    assert_eq!(incoming.len(), 1);

    let (_rel, from_node) = &incoming[0];
    let from_id_raw: i64 = from_node.id().clone().into();
    assert_eq!(from_id_raw, a_id);

    Ok(())
}

#[tokio::test]
async fn repo_incoming_to_skips_wrong_from_labels() -> Result<()> {
    use crate::common::{AB};

    let backend = InMemoryBackend::new();

    let b_id: i64 = {
        let mut tx = backend.begin_tx().await?;
        let x = tx.create_node(vec!["X".to_string()], Default::default()).await?; // WRONG label (should be A)
        let b = tx.create_node(vec!["B".to_string()], Default::default()).await?;
        tx.create_relationship(x.id, b.id, AB::TYPE.to_string(), Default::default()).await?;
        tx.commit().await?;
        b.id
    };

    let repo = RelRepository::<_, AB>::new(backend);

    let incoming = repo.incoming_to(&b_id.into()).await?;
    assert!(incoming.is_empty(), "should skip nodes whose labels don’t match R::From");

    Ok(())
}

