mod common;

#[cfg(test)]
mod tests {
    use crate::common::*;

    use serde_json::json;

    use grm_rs::backend::InMemoryBackend;
    use grm_rs::dsl::{Direction, MatchClause, Return};
    use grm_rs::{
        CompareOp, GrmError, NodeModel, NodePattern, NodeRepository, Query, QueryKind, RelModel,
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

    #[test]
    fn nodepattern_new_has_empty_traversals() {
        let p = NodePattern::<User>::new();
        assert!(p.traversals.is_empty());
        assert_eq!(p.labels, User::LABELS);
        assert!(p.alias.is_none());
        assert!(p.id.is_none());
        assert!(p.property_filters.is_empty());
    }

    #[test]
    fn traversal_builder_pushes_step() {
        let p = NodePattern::<User>::new().out::<Authored>().to::<Post>();

        assert_eq!(p.traversals.len(), 1);
        let step = &p.traversals[0];
        assert!(matches!(step.dir, Direction::Out));
        assert_eq!(step.rel_type, Some(Authored::TYPE));
        assert_eq!(step.end_labels, Post::LABELS);
        assert!(step.end_filters.is_empty());
        assert!(step.end_alias.is_none());
    }

    #[test]
    fn compile_to_graph_root_only() {
        let p = NodePattern::<User>::new().filter(User::name_prop().contains("Ali"));

        let q = Query::matching(p).limit(10).offset(5);
        let g = q.compile_to_graph();

        assert_eq!(g.limit, Some(10));
        assert_eq!(g.offset, Some(5));
        assert!(matches!(g.ret, Return::Node(_)));

        // Expect exactly one Node match clause.
        assert_eq!(g.matches.len(), 1);
        match &g.matches[0] {
            MatchClause::Node(nm) => {
                assert_eq!(nm.labels, User::LABELS);
                assert!(nm.id_filter.is_none());
                assert_eq!(nm.property_filters.len(), 1);
                assert_eq!(nm.property_filters[0].key, "name");
            }
            other => panic!("expected root MatchClause::Node, got: {:?}", other),
        }
    }

    #[test]
    fn compile_to_graph_single_hop_with_end_filters() {
        let p = NodePattern::<User>::new()
            .filter(User::name_prop().contains("Ali"))
            .out::<Authored>()
            .to_where::<Post>(|p| p.filter(Post::title_prop().contains("Rust")));

        let q = Query::matching(p);
        let g = q.compile_to_graph();

        // root node + hop + end node (because end filters exist)
        assert_eq!(g.matches.len(), 3);

        // 0: root node match
        let root_var = match &g.matches[0] {
            MatchClause::Node(nm) => {
                assert_eq!(nm.labels, User::LABELS);
                assert_eq!(nm.property_filters.len(), 1);
                nm.var
            }
            other => panic!("expected root node match, got {:?}", other),
        };

        // 1: hop match
        let end_var = match &g.matches[1] {
            MatchClause::Hop(h) => {
                assert_eq!(h.start, root_var);
                assert_eq!(h.rel_type, Some(Authored::TYPE));
                assert!(matches!(h.dir, Direction::Out));
                assert_eq!(h.end_labels, Post::LABELS);
                h.end
            }
            other => panic!("expected hop match, got {:?}", other),
        };

        // 2: end node match with filter
        match &g.matches[2] {
            MatchClause::Node(nm) => {
                assert_eq!(nm.var, end_var);
                assert_eq!(nm.labels, Post::LABELS);
                assert_eq!(nm.property_filters.len(), 1);
                assert_eq!(nm.property_filters[0].key, "title");
            }
            other => panic!("expected end node match, got {:?}", other),
        };

        // Return should be root var (User)
        debug_assert!(matches!(g.ret, Return::Node(_)));
        let Return::Node(v) = g.ret;
        assert_eq!(v, root_var);
    }

    #[test]
    fn compile_to_graph_multihop_chains_correctly() {
        let p = NodePattern::<User>::new()
            .out::<Authored>()
            .to::<Post>()
            .out::<Authored>()
            .to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        // Option A: root node + (hop + end node) + (hop + end node)
        assert_eq!(g.matches.len(), 5);

        let root_var = match &g.matches[0] {
            MatchClause::Node(nm) => nm.var,
            _ => panic!("expected root node"),
        };

        let hop1_end = match &g.matches[1] {
            MatchClause::Hop(h) => {
                assert_eq!(h.start, root_var);
                h.end
            }
            _ => panic!("expected hop1"),
        };

        // end node match for hop1
        let end1_var = match &g.matches[2] {
            MatchClause::Node(nm) => nm.var,
            _ => panic!("expected end1 node match"),
        };
        assert_eq!(
            end1_var, hop1_end,
            "end1 NodeMatch should target hop1 end var"
        );

        let hop2_end = match &g.matches[3] {
            MatchClause::Hop(h) => {
                assert_eq!(h.start, hop1_end, "hop2 should start at hop1 end");
                h.end
            }
            _ => panic!("expected hop2"),
        };

        // end node match for hop2
        let end2_var = match &g.matches[4] {
            MatchClause::Node(nm) => nm.var,
            _ => panic!("expected end2 node match"),
        };
        assert_eq!(
            end2_var, hop2_end,
            "end2 NodeMatch should target hop2 end var"
        );
    }

    #[test]
    fn compile_preserves_root_id_filter() {
        let p = NodePattern::<User>::new().with_id(UserId(123_i64));
        let g = Query::matching(p).compile_to_graph();

        match &g.matches[0] {
            MatchClause::Node(nm) => assert_eq!(nm.id_filter, Some(123)),
            _ => panic!("expected root node match"),
        }
    }

    #[test]
    fn traversal_builder_incoming_sets_direction_in() {
        let p = NodePattern::<User>::new()
            .incoming::<Authored>()
            .to::<Post>();

        assert_eq!(p.traversals.len(), 1);
        assert!(matches!(p.traversals[0].dir, Direction::In));
    }

    #[test]
    fn traversal_builder_both_sets_direction_both() {
        let p = NodePattern::<User>::new().both::<Authored>().to::<Post>();

        assert_eq!(p.traversals.len(), 1);
        assert!(matches!(p.traversals[0].dir, Direction::Both));
    }

    #[test]
    fn compile_to_graph_preserves_incoming_direction() {
        let p = NodePattern::<User>::new()
            .incoming::<Authored>()
            .to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        match &g.matches[1] {
            MatchClause::Hop(h) => assert!(matches!(h.dir, Direction::In)),
            other => panic!("expected hop match, got {:?}", other),
        }
    }

    #[test]
    fn compile_to_graph_preserves_both_direction() {
        let p = NodePattern::<User>::new().both::<Authored>().to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        match &g.matches[1] {
            MatchClause::Hop(h) => assert!(matches!(h.dir, Direction::Both)),
            other => panic!("expected hop match, got {:?}", other),
        }
    }

    #[test]
    fn compile_to_graph_multihop_incoming_chains_correctly() {
        // (User)<-[:AUTHORED]-(Post)<-[:AUTHORED]-(Post) also silly but good for chaining shape
        let p = NodePattern::<User>::new()
            .incoming::<Authored>()
            .to::<Post>()
            .incoming::<Authored>()
            .to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        // Option A: root node + (hop + end node) + (hop + end node)
        assert_eq!(g.matches.len(), 5);

        let root_var = match &g.matches[0] {
            MatchClause::Node(nm) => nm.var,
            _ => panic!("expected root node"),
        };

        let hop1_end = match &g.matches[1] {
            MatchClause::Hop(h) => {
                assert_eq!(h.start, root_var);
                assert_eq!(h.dir, Direction::In);
                h.end
            }
            _ => panic!("expected hop1"),
        };

        let end1_var = match &g.matches[2] {
            MatchClause::Node(nm) => nm.var,
            _ => panic!("expected end1 node match"),
        };
        assert_eq!(end1_var, hop1_end);

        let hop2_end = match &g.matches[3] {
            MatchClause::Hop(h) => {
                assert_eq!(h.start, hop1_end, "hop2 should start at hop1 end");
                assert_eq!(h.dir, Direction::In);
                h.end
            }
            _ => panic!("expected hop2"),
        };

        let end2_var = match &g.matches[4] {
            MatchClause::Node(nm) => nm.var,
            _ => panic!("expected end2 node match"),
        };
        assert_eq!(end2_var, hop2_end);
    }

    #[test]
    fn compile_to_graph_out_any_sets_rel_type_none() {
        let p = NodePattern::<User>::new().out_any().to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        // With Option A end NodeMatch emission:
        // root node, hop, end node
        assert_eq!(g.matches.len(), 3);

        match &g.matches[1] {
            MatchClause::Hop(h) => {
                assert_eq!(h.dir, Direction::Out);
                assert!(
                    h.rel_type.is_none(),
                    "out_any should compile to HopMatch.rel_type = None"
                );
            }
            _ => panic!("expected hop"),
        }
    }
}
