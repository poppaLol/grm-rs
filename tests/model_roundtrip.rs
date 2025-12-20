mod common;
use crate::common::*;

use grm_rs::{self, NodeModel};

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
