mod common;

use grm_rs::InMemoryBackend;
use grm_rs::dsl::{NodePattern, Query};
use grm_rs::repo::NodeRepository;
use grm_rs::{GraphBackend, GraphTx, NodeModel, RelModel, Result};

use crate::common::{A, AC, AId, C};

#[tokio::test]
async fn repo_query_return_end_returns_end_nodes_not_root() -> Result<()> {
    let backend = InMemoryBackend::new();

    // ---- Arrange: create (A)-[:AB]->(B) ----
    let (a_id, c_id): (i64, i64) = {
        let mut tx = backend.begin_tx().await?;
        let a = tx
            .create_node(vec!["A".to_string()], Default::default())
            .await?;
        let c = tx
            .create_node(vec!["C".to_string()], Default::default())
            .await?;
        tx.create_relationship(a.id, c.id, AC::TYPE, Default::default())
            .await?;
        tx.commit().await?;
        (a.id, c.id)
    };

    // ---- Act: query rooted at A, traverse AC to C, but return END ----
    let q = Query::<A>::matching(
        NodePattern::<A>::new()
            .with_id(AId(a_id))
            .out::<AC>()
            .to::<C>(),
    )
    .return_end();

    // Execute via the *C* repository so decode target is the end node model.
    let repo = NodeRepository::<_, C>::new(backend);

    let results: Vec<C> = repo.fetch(q).await?;

    // ---- Assert: returned nodes are B (the end), and include the created b_id ----
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id().0, c_id);

    Ok(())
}
