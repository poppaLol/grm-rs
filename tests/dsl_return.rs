mod common;

#[cfg(test)]
mod tests {
    use grm_rs::{
        GraphQuery,
        dsl::{MatchClause, Return, VarId},
    };

    use crate::common::*;

    fn node_vars(q: &GraphQuery) -> Vec<VarId> {
        q.matches
            .iter()
            .filter_map(|m| match m {
                MatchClause::Node(nm) => Some(nm.var),
                _ => None,
            })
            .collect()
    }

    fn ret_node_var(q: &GraphQuery) -> VarId {
        match &q.ret {
            Return::Node(v) => *v,
            _ => panic!("Expected Return::Node"),
        }
    }

    #[test]
    fn compile_default_return_is_root_node() {
        use grm_rs::dsl::{NodePattern, Query};

        let q = Query::<User>::matching(NodePattern::<User>::new());
        let gq = q.compile_to_graph();

        let vars = node_vars(&gq);
        assert!(!vars.is_empty(), "Expected at least one NodeMatch");
        let root = *vars.first().unwrap();

        assert_eq!(ret_node_var(&gq), root);
    }

    #[test]
    fn compile_return_end_returns_last_node_var_multihop() {
        use grm_rs::dsl::{NodePattern, Query};

        // Build a multi-hop pattern.
        // Example shape:
        // User -[Authored]-> Post -[SomeRel]-> Tag (or similar)
        let pattern = NodePattern::<User>::new()
            .out::<Authored>() // hop 1
            .to::<Post>() // end node 1
            .out_any() // hop 2 wildcard (or .out::<SomeRel>())
            .to::<User>(); // end node 2 (pick any node model you have)

        let q = Query::<User>::matching(pattern).return_end();
        let gq = q.compile_to_graph();

        let vars = node_vars(&gq);
        assert!(
            vars.len() >= 2,
            "Expected at least 2 NodeMatch vars for a multi-hop traversal"
        );

        let end = *vars.last().unwrap();
        assert_eq!(ret_node_var(&gq), end);
    }

    #[test]
    fn compile_return_end_no_hops_is_same_as_root() {
        use grm_rs::dsl::{NodePattern, Query};

        let q = Query::<User>::matching(NodePattern::<User>::new()).return_end();
        let gq = q.compile_to_graph();

        let vars = node_vars(&gq);
        assert!(!vars.is_empty(), "Expected at least one NodeMatch");
        let root = *vars.first().unwrap();

        assert_eq!(ret_node_var(&gq), root);
    }
}
