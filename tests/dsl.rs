mod common;

#[cfg(test)]
mod tests {
    use crate::common::*;
    use serde_json::json;

    use grm_rs::{
        CompareOp, GrmError, NodePattern, NodeRepository, Query, QueryKind, backend::InMemoryBackend
    };

    #[test]
    fn build_basic_node_pattern() {
        let pattern = NodePattern::<User>::new()
            .alias("u")
            .with_id(UserId(42))
            .filter(User::name_prop().eq("Alice"));

        assert_eq!(pattern.labels, &["User"]);
        assert_eq!(pattern.primary_label(), "User");
        assert_eq!(pattern.alias.as_deref(), Some("u"));
        assert_eq!(pattern.id, Some(UserId(42)));
        assert_eq!(pattern.property_filters.len(), 1);

        let f = &pattern.property_filters[0];
        assert_eq!(f.key, "name");
        assert_eq!(f.op, CompareOp::Eq);
        assert_eq!(f.value, json!("Alice"));
    }

    #[test]
    fn build_match_query_from_node_pattern() {
        // Build a node pattern
        let pattern = NodePattern::<User>::new()
            .alias("u")
            .with_id(UserId(42))
            .filter(User::name_prop().eq("Alice"));

        // Wrap into a Query and set limit/offset
        let q = Query::matching(pattern).limit(10).offset(5);

        // Inspect the structure
        match &q.kind {
            QueryKind::MatchNode {
                pattern,
                limit,
                offset,
            } => {
                // labels + alias + id
                assert_eq!(pattern.labels, &["User"]);
                assert_eq!(pattern.alias.as_deref(), Some("u"));
                assert_eq!(pattern.id, Some(UserId(42)));

                // property filter
                assert_eq!(pattern.property_filters.len(), 1);
                let f = &pattern.property_filters[0];
                assert_eq!(f.key, "name");
                assert_eq!(f.op, CompareOp::Eq);
                assert_eq!(f.value, json!("Alice"));

                // paging
                assert_eq!(*limit, Some(10));
                assert_eq!(*offset, Some(5));
            }
        }

        // Helper accessors
        assert_eq!(q.limit_value(), Some(10));
        assert_eq!(q.offset_value(), Some(5));
    }

    #[tokio::test]
    async fn query_users_by_eq_filter_on_name() -> Result<(), GrmError> {
        let backend = InMemoryBackend::new();
        let repo: NodeRepository<_, User> = NodeRepository::new(backend.clone());

        // Seed some data via the existing repository API.
        let mut u1 = User {
            id: UserId(0),
            name: "Alice".into(),
            age: 30,
        };
        let mut u2 = User {
            id: UserId(0),
            name: "Bob".into(),
            age: 40,
        };
        let mut u3 = User {
            id: UserId(0),
            name: "Alicia".into(),
            age: 25,
        };

        repo.create(&mut u1).await?;
        repo.create(&mut u2).await?;
        repo.create(&mut u3).await?;

        // Sanity: ids should be set by the backend
        assert!(u1.id != UserId(0));
        assert!(u2.id != UserId(0));
        assert!(u3.id != UserId(0));

        // Build a NodePattern that matches users whose name CONTAINS "Ali".
        let pattern = NodePattern::<User>::new()
            .alias("u")
            .filter(User::name_prop().eq("Alice"));

        // Wrap into a Query with a limit
        let q = Query::matching(pattern).limit(10);

        // Execute via the new DSL-driven entrypoint
        let mut users = repo.query(q).await?;

        // We expect Alice + Alicia, but not Bob
        users.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
        assert_eq!(names, vec!["Alice".to_string()]);

        Ok(())
    }

    #[tokio::test]
    async fn query_user_by_id_and_filters() -> Result<(), GrmError> {
        let backend = InMemoryBackend::new();
        let repo: NodeRepository<_, User> = NodeRepository::new(backend.clone());

        let mut u1 = User {
            id: UserId(0),
            name: "Charlie".into(),
            age: 50,
        };
        let mut u2 = User {
            id: UserId(0),
            name: "Dave".into(),
            age: 60,
        };

        repo.create(&mut u1).await?;
        repo.create(&mut u2).await?;

        // Build a pattern constrained by ID and an extra filter on age
        let pattern = NodePattern::<User>::new()
            .with_id(u1.id)
            .filter(User::age_prop().gt(40_i32));

        let q = Query::matching(pattern);

        let users = repo.query(q).await?;

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "Charlie");

        Ok(())
    }
}
