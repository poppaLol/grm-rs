mod common;

#[cfg(test)]
mod node_matches_filters_tests {
    use crate::common::*;
    use grm_rs::{CompareOp, dsl::props_match_filters};

    use grm_rs::{GraphClient, InMemoryBackend, NodeModel, PropertyFilter, Result};
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
}