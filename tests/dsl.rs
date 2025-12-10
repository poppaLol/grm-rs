mod common;

#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::common::*;

    use grm_rs::{Property, NodePattern, CompareOp}; // adjust path
    impl User {
        // stand-ins for what the macro will generate later
        fn name_prop() -> Property<User, String> {
            Property::new("name")
        }
    }

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
}
