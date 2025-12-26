mod common;

#[cfg(test)]
mod node_matches_filters_tests {
    use crate::common::*;
    use grm_rs::{CompareOp, dsl::props_match_filters};

    use grm_rs::{
        GraphClient, InMemoryBackend, NodeModel, NodePattern, PropertyFilter, Query, Result,
    };
    use serde_json::json;

    #[test]
    fn single_failing_filter_should_reject_node() {
        let user = User {
            id: UserId(1),
            name: "Alice".to_string(),
            age: 20,
        };

        // This filter should clearly fail for "Alice"
        let filters = vec![PropertyFilter {
            key: "name",
            op: CompareOp::Eq,
            value: json!("Bob"),
        }];

        let props = user.to_properties();
        let matches = props_match_filters(&props, &filters);

        assert!(
            !matches,
            "expected user NOT to match filter name == \"Bob\", \
             but props_matches_filters returned true"
        );
    }

    #[test]
    fn multiple_filters_should_be_and_combined() {
        let user = User {
            id: UserId(1),
            name: "Alice".to_string(),
            age: 20,
        };

        // These filters are contradictory: can't be both Alice and Bob.
        let filters = vec![
            PropertyFilter {
                key: "name",
                op: CompareOp::Eq,
                value: json!("Alice"),
            },
            PropertyFilter {
                key: "name",
                op: CompareOp::Eq,
                value: json!("Bob"),
            },
        ];

        let props = user.to_properties();
        let matches = props_match_filters(&props, &filters);

        // With the buggy logic, this will also be TRUE,
        // because failures just `continue` instead of rejecting the node.
        assert!(
            !matches,
            "expected user NOT to match filters name == \"Alice\" AND name == \"Bob\", \
             but node_matches_filters returned true"
        );
    }

    #[tokio::test]
    async fn repo_is_short_lived_so_commit_is_possible() -> Result<()> {
        let backend = InMemoryBackend::new();
        let client = GraphClient::new(backend);

        let mut tx = client.transaction().await?;

        // Create the repo and drop it immediately
        let _repo = tx.repo();
        drop(_repo);

        // If repo borrowing was wrong, this would not compile or would fail to commit
        tx.commit().await?;
        Ok(())
    }

    #[tokio::test]
    async fn tx_scoped_node_repo_crud_works() -> Result<()> {
        let backend = InMemoryBackend::new();
        let client = GraphClient::new(backend);

        // Start a unit-of-work
        let mut tx = client.transaction().await?;

        // IMPORTANT: scope the repo borrow so we can commit afterwards
        // we use an anonymous scope to ensure this all happens together
        {
            // Build the single "graph handle"
            let mut repo = tx.repo();

            // Get a typed node repo (tx-scoped)
            let mut users = repo.nodes::<User>();

            // --- create ---
            let mut user = User {
                // fill in your struct fields; id likely starts as default/empty
                name: "alice".to_string(),
                age: 30,
                id: UserId::default(),
            };

            users.create(&mut user).await?;

            // You must have an id now
            let id = user.id().clone();

            // --- find_by_id ---
            let fetched = users.find_by_id(&id).await?;
            assert!(fetched.is_some());
            assert_eq!(fetched.as_ref().unwrap().name, "alice");

            // --- update ---
            // change a field and persist
            let mut updated = fetched.unwrap();
            updated.age = 31;
            users.update(&updated).await?;

            // --- find_by(property) ---
            // this assumes your backend stores properties in JSON and supports property equality
            let matches = users.find_by("name", &json!("alice")).await?;
            assert!(!matches.is_empty());
            assert!(matches.iter().any(|u| u.id() == &id));
        }

        // Commit once
        tx.commit().await?;
        Ok(())
    }

    #[tokio::test]
    async fn repo_facade_executes_node_and_rel_queries_in_a_single_transaction() -> Result<()> {
        let backend = InMemoryBackend::new();
        let client = GraphClient::new(backend);

        let mut tx = client.transaction().await?;

        // --- Arrange: create a user + post + relationship in one transaction ---
        let mut user = User {
            name: "alice".into(),
            age: 30,
            id: UserId::default(), // will be set by create()
        };

        let mut post = Post {
            title: "hello world".into(),
            id: PostId::default(), // will be set by create()
        };

        // Create entities (short-lived repo use)
        {
            let mut repo = tx.repo();
            repo.nodes::<User>().create(&mut user).await?;
            repo.nodes::<Post>().create(&mut post).await?;

            let user_id = user.id().clone();
            let post_id = post.id().clone();

            let mut authored = Authored {
                id: AuthoredId::default(),
                year: 2020,
            };
            repo.rels::<Authored>()
                .create_between(&user_id, &post_id, &mut authored)
                .await?;
        }

        // --- Act + Assert: use the facade to query nodes + rels ---
        {
            let mut repo = tx.repo();

            let q_users =
                Query::<User>::matching(NodePattern::<User>::new().out::<Authored>().to::<Post>());

            let users_found: Vec<User> = repo.query(q_users).await?;
            assert!(users_found.iter().any(|u| u.name == "alice"));

            let q_rels =
                Query::<User>::matching(NodePattern::<User>::new().out::<Authored>().to::<Post>())
                    .return_rel();

            let rels_found: Vec<Authored> = repo.query_rel::<User, Authored>(q_rels).await?;
            assert_eq!(rels_found.len(), 1);
            assert_eq!(rels_found[0].year, 2020);
        }

        // Commit once at the end
        tx.commit().await?;
        Ok(())
    }
}
