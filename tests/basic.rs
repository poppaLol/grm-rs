mod common;

#[cfg(test)]
mod tests {
    use crate::common::*;
    use grm_rs;
    use grm_rs::{
        GraphBackend, GraphTx, NodeModel, NodeRepository, RelRepository, backend::InMemoryBackend,
    };
    use serde_json::json;

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

        //
        // CREATE a node
        //
        let create_result = backend
            .execute_query(
                "CREATE (n:User { name: $name }) RETURN n",
                json!({
                    "labels": ["User"],
                    "props": { "name": "Alice" }
                }),
            )
            .await
            .expect("create failed");

        assert_eq!(create_result.rows.len(), 1);

        let node_json = &create_result.rows[0].values["n"];
        let created_id = node_json["id"].as_i64().unwrap();
        assert_eq!(node_json["props"]["name"], "Alice");

        //
        // MATCH (n) WHERE id(n) = $id RETURN n
        //
        let match_result = backend
            .execute_query(
                "MATCH (n) WHERE id(n) = $id RETURN n",
                json!({ "id": created_id }),
            )
            .await
            .expect("match failed");

        assert_eq!(match_result.rows.len(), 1);
        let matched_node = &match_result.rows[0].values["n"];

        assert_eq!(matched_node["id"].as_i64().unwrap(), created_id);
        assert_eq!(matched_node["props"]["name"], "Alice");
    }

    #[tokio::test]
    async fn node_repository_create_and_find() {
        let backend = InMemoryBackend::new();
        let repo = NodeRepository::<_, User>::new(backend);

        // 1. CREATE
        let mut user = User {
            id: UserId(0),
            name: "Alice".into(),
            age: 0,
        };

        repo.create(&mut user).await.unwrap();
        assert!(user.id.0 > 0, "Backend should assign a non-zero ID");

        // 2. FIND
        let found = repo
            .find_by_id(&user.id)
            .await
            .unwrap()
            .expect("Expected to find user");

        // 3. ASSERT equality
        assert_eq!(found.name, user.name);
        assert_eq!(found.id, user.id);
    }

    #[tokio::test]
    async fn node_repository_update() {
        use grm_rs::NodeRepository;
        use grm_rs::backend::InMemoryBackend;

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
        user.name = "Bob".into();
        repo.update(&user).await.unwrap();

        // Fetch again
        let fetched = repo
            .find_by_id(&user.id)
            .await
            .unwrap()
            .expect("user should exist after update");

        assert_eq!(fetched.id, user.id);
        assert_eq!(fetched.name, "Bob");
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

        // Start a transaction
        let mut tx = backend.begin_tx().await.expect("begin_tx failed");

        // 1. Create a node inside the transaction
        let create_res = tx
            .execute_query(
                "CREATE (n:User { name: $name }) RETURN n",
                json!({
                    "labels": ["User"],
                    "props": { "name": "TempUser" }
                }),
            )
            .await
            .expect("create in tx failed");

        assert_eq!(create_res.rows.len(), 1);

        let node_json = &create_res.rows[0].values["n"];
        let temp_id = node_json["id"].as_i64().expect("id not i64");

        // 2. Cause an error inside the same transaction (unsupported query)
        let err = tx
            .execute_query("XXXX THIS IS NOT VALID CYPHER XXXX", json!({}))
            .await;

        assert!(err.is_err(), "expected invalid query to fail");

        // 3. Roll the transaction back
        tx.rollback().await.expect("rollback failed");

        // 4. Verify the node created inside the tx does NOT exist globally
        let match_res = backend
            .execute_query(
                "MATCH (n) WHERE id(n) = $id RETURN n",
                json!({ "id": temp_id }),
            )
            .await
            .expect("match outside tx failed");

        assert_eq!(
            match_res.rows.len(),
            0,
            "node created in rolled-back tx should not be visible"
        );
    }
}
