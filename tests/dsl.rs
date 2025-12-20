mod common;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::common::*;

    use serde_json::json;

    use grm_rs::backend::InMemoryBackend;
    use grm_rs::dsl::{Direction, KernelValue, MatchClause, Return};
    use grm_rs::{
        CompareOp, GraphBackend, GraphTx, NodeModel, NodePattern, NodeRepository, Query, QueryKind,
        RelModel, RelRepository, Result,
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
    async fn query_users_by_eq_filter_on_name() -> Result<()> {
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
        let mut users = repo.fetch(q).await?;

        // We expect Alice + Alicia, but not Bob
        users.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
        assert_eq!(names, vec!["Alice".to_string()]);

        Ok(())
    }

    #[tokio::test]
    async fn query_user_by_id_and_filters() -> Result<()> {
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

        let users = repo.fetch(q).await?;

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
        match g.ret {
            Return::Node(v) => assert_eq!(v, root_var),
            other => panic!("expected Return::Node, got {:?}", other),
        }
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

    #[test]
    fn compile_to_graph_incoming_any_sets_rel_type_none() {
        let p = NodePattern::<User>::new().incoming_any().to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        // Option A invariant: root node, hop, end node
        assert_eq!(g.matches.len(), 3);

        match &g.matches[1] {
            MatchClause::Hop(h) => {
                assert_eq!(h.dir, Direction::In);
                assert!(
                    h.rel_type.is_none(),
                    "incoming_any should compile to HopMatch.rel_type = None"
                );
            }
            _ => panic!("expected hop"),
        }
    }

    #[test]
    fn compile_to_graph_both_any_sets_rel_type_none() {
        let p = NodePattern::<User>::new().both_any().to::<Post>();

        let g = Query::matching(p).compile_to_graph();

        // Option A invariant: root node, hop, end node
        assert_eq!(g.matches.len(), 3);

        match &g.matches[1] {
            MatchClause::Hop(h) => {
                assert_eq!(h.dir, Direction::Both);
                assert!(
                    h.rel_type.is_none(),
                    "both_any should compile to HopMatch.rel_type = None"
                );
            }
            _ => panic!("expected hop"),
        }
    }

    #[tokio::test]
    async fn execute_graph_incoming_any_matches_any_relationship_type() -> Result<()> {
        let backend = InMemoryBackend::new();

        // Make: (Post)-[:LIKED]->(User)
        let user_id: i64;
        {
            let mut tx = backend.begin_tx().await?;
            let u = tx
                .create_node(vec!["User".to_string()], Default::default())
                .await?;
            let p = tx
                .create_node(vec!["Post".to_string()], Default::default())
                .await?;
            tx.create_relationship(p.id, u.id, "LIKED".to_string(), Default::default())
                .await?;
            user_id = u.id;
            tx.commit().await?;
        }

        // Query: match (User)<-[*]-(Post)
        let q = Query::<User>::matching(NodePattern::<User>::new().incoming_any().to::<Post>());
        let gq = q.compile_to_graph();

        let mut tx = backend.begin_tx().await?;
        let qr = tx.execute_graph(&gq).await?;
        tx.commit().await?;

        let got_ids: BTreeSet<i64> = qr
            .rows
            .iter()
            .filter_map(|row| {
                row.values.values().next().and_then(|v| match v {
                    KernelValue::Node(n) => Some(n.id),
                    __ => panic!("expected rel"),
                })
            })
            .collect();

        assert!(
            got_ids.contains(&user_id),
            "incoming_any should match User via any rel type"
        );

        Ok(())
    }

    #[tokio::test]
    async fn execute_graph_both_any_matches_incoming_or_outgoing_relationships() -> Result<()> {
        let backend = InMemoryBackend::new();

        // Make:
        //   (User)-[:BOOKMARKED]->(Post1)
        //   (Post2)-[:LIKED]->(User)
        // both_any from User to Post should match either direction, and still return the User root.
        let user_id: i64;
        {
            let mut tx = backend.begin_tx().await?;
            let u = tx
                .create_node(vec!["User".to_string()], Default::default())
                .await?;
            let p1 = tx
                .create_node(vec!["Post".to_string()], Default::default())
                .await?;
            let p2 = tx
                .create_node(vec!["Post".to_string()], Default::default())
                .await?;

            tx.create_relationship(u.id, p1.id, "BOOKMARKED".to_string(), Default::default())
                .await?;
            tx.create_relationship(p2.id, u.id, "LIKED".to_string(), Default::default())
                .await?;

            user_id = u.id;
            tx.commit().await?;
        }

        let q = Query::<User>::matching(NodePattern::<User>::new().both_any().to::<Post>());
        let gq = q.compile_to_graph();

        let mut tx = backend.begin_tx().await?;
        let qr = tx.execute_graph(&gq).await?;
        tx.commit().await?;

        let got_ids: BTreeSet<i64> = qr
            .rows
            .iter()
            .filter_map(|row| {
                row.values.values().next().and_then(|v| match v {
                    KernelValue::Node(n) => Some(n.id),
                    _ => panic!("expected rel"),
                })
            })
            .collect();

        assert!(
            got_ids.contains(&user_id),
            "both_any should match User via incoming or outgoing rels"
        );

        Ok(())
    }

    #[tokio::test]
    async fn return_relationship_from_traversal() -> Result<()> {
        let backend = InMemoryBackend::new();
        let u_repo = NodeRepository::<_, User>::new(backend.clone());
        let p_repo = NodeRepository::<_, Post>::new(backend.clone());
        let authored = RelRepository::<_, Authored>::new(backend.clone());

        // Seed some data via the existing repository API.
        let mut u1 = User {
            id: UserId(0),
            name: "Alice".into(),
            age: 30,
        };
        let mut p1 = Post {
            id: PostId(0),
            title: "Hello".into(),
        };
        let mut auth = Authored {
            id: AuthoredId::default(),
            year: 2024,
        };

        u_repo.create(&mut u1).await?;
        p_repo.create(&mut p1).await?;
        // create relationship
        authored.create_between(&u1.id, &p1.id, &mut auth).await?;

        // query: User -[AUTHORED]-> Post, return rel
        let q = Query::<User>::matching(
            NodePattern::<User>::new()
                .filter(User::name_prop().eq("Alice"))
                .out::<Authored>()
                .to::<Post>(),
        )
        .return_rel();
        
        // need to query this from the user repo for now, but will
        // prefer arbitrary node or rel repo in future
        let exec = u_repo.execute(q).await?;
        let (gq, qr) = (exec.gq, exec.qr);
        assert_eq!(qr.rows.len(), 1);

        match qr.rows[0].get_returned(&gq).unwrap() {
            KernelValue::Rel(r) => assert_eq!(r.ty, "AUTHORED"),
            _ => panic!("expected rel"),
        }

        Ok(())
    }
}
