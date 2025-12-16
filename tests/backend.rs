mod common;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use grm_rs::dsl::{GraphQuery, KernelValue, MatchClause, NodeMatch, Return};
    use serde_json::json;

    use crate::common::*;

    use grm_rs::{self, NodePattern, Query, VarGen};
    use grm_rs::{
        GraphBackend, GraphTx, InMemoryBackend, NodeModel, NodeRepository, RelRepository, Result,
    };

    #[test]
    fn models_compile_and_roundtrip() {
        let user = User {
            id: UserId(1),
            name: "Alice".into(),
            age: 0,
        };
        let props = user.to_properties();
        let user2 = User::from_properties(UserId(1), props).unwrap();
        assert_eq!(user2.id, UserId(1));
        assert_eq!(user2.name, "Alice");
    }

    #[tokio::test]
    async fn in_memory_backend_create_and_match_node() {
        let backend = InMemoryBackend::new();
        let mut tx = backend.begin_tx().await.expect("begin tx failed");

        // CREATE a node (typed)
        let mut props = BTreeMap::new();
        props.insert("name".to_string(), json!("Alice"));

        let node = tx
            .create_node(vec!["User".to_string()], props)
            .await
            .expect("create_node failed");

        let created_id = node.id;
        assert_eq!(node.props.get("name").unwrap(), &json!("Alice"));

        // MATCH by id (typed)
        let found = tx
            .find_node_by_id(created_id)
            .await
            .expect("find_node_by_id failed")
            .expect("node not found");

        assert_eq!(found.id, created_id);
        assert_eq!(found.props.get("name").unwrap(), &json!("Alice"));

        tx.commit().await.expect("commit failed");
    }

    #[tokio::test]
    async fn in_memory_backend_create_and_match_node_via_graphquery() {
        let backend = InMemoryBackend::new();
        let mut tx = backend.begin_tx().await.expect("begin tx failed");

        // create node
        let mut props = BTreeMap::new();
        props.insert("name".to_string(), json!("Alice"));
        let node = tx
            .create_node(vec!["User".to_string()], props)
            .await
            .expect("create_node failed");
        let created_id = node.id;

        tx.commit().await.expect("commit failed");

        // build GraphQuery that matches by id
        let mut vg = VarGen::default();
        let root = vg.fresh();

        let gq = GraphQuery {
            matches: vec![MatchClause::Node(NodeMatch {
                var: root,
                labels: &["User"],
                id_filter: Some(created_id),
                property_filters: vec![],
            })],
            where_: vec![],
            ret: Return::Node(root),
            limit: None,
            offset: None,
        };

        let qr = backend
            .execute_graph(&gq)
            .await
            .expect("execute_graph failed");
        assert_eq!(qr.rows.len(), 1);

        let node = qr.rows[0].get_returned(&gq).unwrap().as_node().unwrap();

        assert_eq!(node.id, created_id);
        assert_eq!(node.props["name"], "Alice");
    }

    #[tokio::test]
    async fn in_memory_backend_create_and_find_by_name() {
        //note this test does not match real world use - you will have duplicate
        //non-unique "name" entries e.g.
        let backend = InMemoryBackend::new();
        let mut tx = backend.begin_tx().await.expect("begin tx failed");

        // 1. CREATE
        let mut props = BTreeMap::new();
        props.insert("name".to_string(), json!("Alice"));
        let node = tx
            .create_node(vec!["User".to_string()], props)
            .await
            .expect("create_node failed");

        tx.commit().await.expect("commit failed");

        assert!(node.id > 0, "Backend should assign a non-zero ID");

        // 2. FIND
        let q = Query::<User>::matching(
            NodePattern::<User>::new().filter(User::name_prop().eq("Alice")),
        );
        let gq = q.compile_to_graph();
        let qr = backend
            .execute_graph(&gq)
            .await
            .expect("execute_graph failed");
        assert_eq!(qr.rows.len(), 1);

        // 3. ASSERT equality
        let node = qr.rows[0].get_returned(&gq).unwrap().as_node().unwrap();

        assert_eq!(node.id, node.id);
        assert_eq!(node.props["name"], "Alice");
    }

    #[tokio::test]
    async fn in_memory_backend_create_and_update() {
        let backend = InMemoryBackend::new();
        let repo = NodeRepository::<_, User>::new(backend);

        // Create a user
        let mut user = User {
            id: UserId(0),
            name: "Alice".into(),
            age: 0,
        };
        repo.create(&mut user).await.unwrap();
        assert!(user.id.0 >= 1);

        // Update name
        user.name = "Bob".to_string();
        repo.update(&user).await.unwrap();

        // Fetch again
        let q =
            Query::<User>::matching(NodePattern::<User>::new().filter(User::name_prop().eq("Bob")));
        let users = repo.query(q).await.expect("query failed");

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].id, user.id);
        assert_eq!(users[0].name, "Bob");
    }

    #[tokio::test]
    async fn node_repository_delete() {
        let backend = InMemoryBackend::new();
        let repo = NodeRepository::<_, User>::new(backend);

        // Create a user
        let mut user = User {
            id: UserId(0),
            name: "Charlie".into(),
            age: 0,
        };
        repo.create(&mut user).await.unwrap();
        let user_id = user.id;

        // Delete
        repo.delete(&user_id).await.unwrap();

        // Ensure it's gone
        let fetched = repo.find_by_id(&user_id).await.unwrap();
        assert!(fetched.is_none(), "user should be deleted");
    }

    #[tokio::test]
    async fn repository_find_by_property() {
        let backend = InMemoryBackend::new();
        let repo = NodeRepository::<_, User>::new(backend);

        // Create two users
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

        // Find by name = Alice
        let results = repo.find_by("name", &json!("Alice")).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Alice");
    }

    #[tokio::test]
    async fn rel_repository_create_and_outgoing() {
        let backend = InMemoryBackend::new();

        let user_repo = NodeRepository::<_, User>::new(backend.clone());
        let post_repo = NodeRepository::<_, Post>::new(backend.clone());
        let rel_repo = RelRepository::<_, Authored>::new(backend.clone());

        // Create a user
        let mut user = User {
            id: UserId(0),
            name: "Alice".into(),
            age: 0,
        };
        user_repo.create(&mut user).await.unwrap();

        // Create a post
        let mut post = Post {
            id: PostId(0),
            title: "Hello Graph".into(),
        };
        post_repo.create(&mut post).await.unwrap();

        // Create AUTHORED relationship: (user)-[AUTHORED]->(post)
        let mut authored = Authored {
            id: AuthoredId(0),
            year: 2024,
        };
        rel_repo
            .create_between(&user.id, &post.id, &mut authored)
            .await
            .unwrap();
        assert!(i64::from(authored.id) >= 1);

        // Traverse outgoing from user
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

        // Create user + post
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

        // Create relationship
        let mut authored = Authored {
            id: AuthoredId(0),
            year: 2024,
        };
        rel_repo
            .create_between(&user.id, &post.id, &mut authored)
            .await
            .unwrap();

        // Sanity: outgoing exists
        let edges_before = rel_repo.outgoing_from(&user.id).await.unwrap();
        assert_eq!(edges_before.len(), 1);

        // Delete user
        user_repo.delete(&user.id).await.unwrap();

        // Outgoing from user ID should now be empty
        let edges_after = rel_repo.outgoing_from(&user.id).await.unwrap();
        assert_eq!(edges_after.len(), 0);
    }

    #[tokio::test]
    async fn transaction_rollback_on_error() {
        let backend = InMemoryBackend::new();

        // 1) Start a transaction
        let mut tx = backend.begin_tx().await.expect("begin_tx failed");

        // 2) Create a node inside the transaction (typed)
        let mut props = BTreeMap::new();
        props.insert("name".to_string(), json!("TempUser"));

        let node = tx
            .create_node(vec!["User".to_string()], props)
            .await
            .expect("create_node in tx failed");

        let temp_id = node.id;

        // 3) Cause an error inside the same transaction
        // In the new world, string queries are unsupported, so this is a natural error source.
        let err = tx.execute_query("XXXX NOT SUPPORTED XXXX", json!({})).await;
        assert!(err.is_err(), "expected unsupported query to fail");

        // 4) Roll back
        tx.rollback().await.expect("rollback failed");

        // 5) Verify the node does NOT exist globally
        let mut tx2 = backend.begin_tx().await.expect("begin_tx failed");
        let found = tx2
            .find_node_by_id(temp_id)
            .await
            .expect("find_node_by_id failed");
        tx2.commit().await.expect("commit failed");

        assert!(
            found.is_none(),
            "node created in rolled-back tx should not be visible"
        );
    }

    #[tokio::test]
    async fn simple_transaction_rollback_discards_changes() {
        let backend = InMemoryBackend::new();

        let mut tx = backend.begin_tx().await.expect("begin_tx failed");

        let mut props = BTreeMap::new();
        props.insert("name".to_string(), json!("TempUser"));

        let node = tx
            .create_node(vec!["User".to_string()], props)
            .await
            .unwrap();
        let temp_id = node.id;

        tx.rollback().await.unwrap();

        let mut tx2 = backend.begin_tx().await.unwrap();
        let found = tx2.find_node_by_id(temp_id).await.unwrap();
        tx2.commit().await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn execute_graph_out_any_matches_any_relationship_type() -> Result<()> {
        // Create: (User)-[:LIKED]->(Post)
        // Then query: match (User)-[*out_any*]->(Post) should return the User

        let backend = InMemoryBackend::new();

        let user_id: i64;
        {
            let mut tx = backend.begin_tx().await?;

            let u = tx
                .create_node(vec!["User".to_string()], Default::default())
                .await?;
            let p = tx
                .create_node(vec!["Post".to_string()], Default::default())
                .await?;

            tx.create_relationship(u.id, p.id, "LIKED".to_string(), Default::default())
                .await?;

            tx.commit().await?;
            user_id = u.id;
        }

        // Build typed query: User.out_any().to::<Post>()
        let q = Query::<User>::matching(NodePattern::<User>::new().out_any().to::<Post>());

        let gq = q.compile_to_graph();

        let mut tx = backend.begin_tx().await?;
        let qr = tx.execute_graph(&gq).await?;
        tx.commit().await?;

        // Query returns nodes indicated by "ret"
        let got_ids: Vec<i64> = qr
            .rows
            .iter()
            .filter_map(|row| {
                row.values.values().next().and_then(|v| match v {
                    KernelValue::Node(n) => Some(n.id),
                    _ => panic!("expected node"),
                })
            })
            .collect();

        assert!(
            got_ids.contains(&user_id),
            "expected out_any traversal to match User via non-typed relationship"
        );

        Ok(())
    }

    #[tokio::test]
    async fn tx_incoming_returns_from_node_for_matching_type() -> Result<()> {
        let backend = InMemoryBackend::new();

        let (a_id, b_id, rel_type) = {
            let mut tx = backend.begin_tx().await?;
            let a = tx
                .create_node(vec!["A".to_string()], Default::default())
                .await?;
            let b = tx
                .create_node(vec!["B".to_string()], Default::default())
                .await?;
            let rel_type = "R".to_string();

            tx.create_relationship(a.id, b.id, rel_type.clone(), Default::default())
                .await?;
            tx.commit().await?;
            (a.id, b.id, rel_type)
        };

        let mut tx = backend.begin_tx().await?;

        // incoming to B should yield (rel, A)
        let incoming_to_b = tx.incoming(b_id, Some(&rel_type)).await?;
        assert_eq!(incoming_to_b.len(), 1);

        let (_rel, from_node) = &incoming_to_b[0];
        assert_eq!(from_node.id, a_id);

        // incoming to A should be empty (no rel ends at A)
        let incoming_to_a = tx.incoming(a_id, Some(&rel_type)).await?;
        assert!(incoming_to_a.is_empty());

        tx.commit().await?;
        Ok(())
    }

    #[tokio::test]
    async fn tx_both_returns_neighbors_from_outgoing_and_incoming() -> Result<()> {
        let backend = InMemoryBackend::new();

        let (a_id, b_id, c_id, rel_type) = {
            let mut tx = backend.begin_tx().await?;

            // Graph shape:
            //   C -[R]-> A -[R]-> B
            let a = tx
                .create_node(vec!["A".to_string()], Default::default())
                .await?;
            let b = tx
                .create_node(vec!["B".to_string()], Default::default())
                .await?;
            let c = tx
                .create_node(vec!["C".to_string()], Default::default())
                .await?;

            let rel_type = "R".to_string();
            tx.create_relationship(c.id, a.id, rel_type.clone(), Default::default())
                .await?;
            tx.create_relationship(a.id, b.id, rel_type.clone(), Default::default())
                .await?;

            tx.commit().await?;
            (a.id, b.id, c.id, rel_type)
        };

        let mut tx = backend.begin_tx().await?;

        let pairs = tx.both(a_id, Some(&rel_type)).await?;

        let neighbor_ids: BTreeSet<i64> = pairs.into_iter().map(|(_rel, n)| n.id).collect();
        let expected: BTreeSet<i64> = [b_id, c_id].into_iter().collect();

        assert_eq!(neighbor_ids, expected);

        tx.commit().await?;
        Ok(())
    }
}
