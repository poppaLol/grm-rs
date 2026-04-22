use std::fmt::Binary;

/*
 * This file contains some sample entities we can use for testing the codebase
 * In each case there is a strongly typed ID
 * e.g. UserId / User. Additionally you should be able to see properties for the fields e.g. name_prop being
 * the reference for name property "title"
 */
use serde::{Deserialize, Serialize};
use grm_rs::{NodeModel, RelModel, typed_id};

typed_id!(UserId);
typed_id!(PostId);
typed_id!(AuthoredId);
typed_id!(BId);
typed_id!(AId);
typed_id!(CId);
typed_id!(ABId);
typed_id!(ACId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct User {
    #[grm(id)]
    #[serde(skip)]
    pub(crate) id: UserId,
    pub name: String,
    pub age: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct Post {
    #[grm(id)]
    #[serde(skip)]
    pub id: PostId,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "User", to = "Post", ty = "AUTHORED")]
pub struct Authored {
    #[grm(id)]
    #[serde(skip)]
    pub id: AuthoredId,
    pub year: u64,
    #[grm(skip)]
    pub(crate) from: UserId,
    #[grm(skip)]
    pub(crate) to: PostId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct A {
    #[grm(id)]
    #[serde(skip)]
    id: AId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct B {
    #[grm(id)]
    #[serde(skip)]
    id: BId,
    // required property so decode can fail
    must_have: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct C {
    #[grm(id)]
    #[serde(skip)]
    id: CId
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "A", to = "B", ty = "AB")]
pub struct AB {
    #[grm(id)]
    #[serde(skip)]
    id: ABId,
    #[grm(skip)]
    from: AId,
    #[grm(skip)]
    to: BId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "A", to = "C", ty = "AC")]
pub struct AC {
    #[grm(id)]
    #[serde(skip)]
    id: ACId,
    #[grm(skip)]
    from: AId,
    #[grm(skip)]
    to: CId,
}