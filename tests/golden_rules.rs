/**
 * The contract - or the Golden Rules for backend implementation
 * 1) `execute_graph(&GraphQuery)` returns a `QueryResult` containing rows (entities and edges more
 *    to be more specific).
 * 2) Each row is a mapping VarId -> KernelValue for all bound vars in the query
 * 3) The row must contain KernelValue for gq.return_var() whose variant matches gq.return_kind().
 */
mod common;
use crate::common::*;
use grm_rs::{
    GraphClient, InMemoryBackend, KernelValue, NodeModel, NodePattern, Query, Result, ReturnKind,
};

#[tokio::test]
async fn execute_graph_rows_contain_bound_vars_and_correct_return_kind() -> Result<()> {
    let backend = InMemoryBackend::new();
    let client = GraphClient::new(backend);
    let mut tx = client.transaction().await?;

    // Arrange: User -[Authored]-> Post
    {
        let mut repo = tx.repo();

        let mut user = User {
            name: "alice".into(),
            age: 30,
            id: UserId::default(),
        };
        let mut post = Post {
            title: "hello".into(),
            id: PostId::default(),
        };
        repo.nodes::<User>().create(&mut user).await?;
        repo.nodes::<Post>().create(&mut post).await?;

        let mut rel = Authored {
            id: AuthoredId::default(),
            year: 2020,
            from: UserId::default(),
            to: PostId::default(),
        };
        repo.rels::<Authored>()
            .create_between(user.id(), post.id(), &mut rel)
            .await?;
    }

    // Query returns the rel
    let q = Query::<User>::matching(NodePattern::<User>::new().out::<Authored>().to::<Post>())
        .return_rel();

    let exec = tx.execute(q).await?;

    // Optional: validate compiled query invariants
    exec.gq.validate()?;

    let bound = exec.gq.bound_vars();
    let ret_var = exec.gq.return_var();
    let ret_kind = exec.gq.return_kind();

    assert!(!exec.qr.rows.is_empty());

    for row in &exec.qr.rows {
        for v in &bound {
            assert!(row.contains_key(v), "row missing bound var {v:?}");
        }

        match ret_kind {
            ReturnKind::Node => assert!(matches!(row.get(&ret_var), Some(KernelValue::Node(_)))),
            ReturnKind::Rel => assert!(matches!(row.get(&ret_var), Some(KernelValue::Rel(_)))),
        }
    }

    tx.commit().await?;
    Ok(())
}
