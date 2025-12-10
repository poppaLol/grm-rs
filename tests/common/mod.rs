use serde::{Serialize, Deserialize};
use grm_rs::{RelModel, typed_id};
use grm_rs::{NodeModel};

typed_id!(UserId);
typed_id!(PostId);
typed_id!(AuthoredId);

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct User {
    #[serde(skip)]
    pub(crate) id: UserId,
    pub name: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, NodeModel)]
pub struct Post {
    #[serde(skip)]
    pub id: PostId,
    pub title: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RelModel)]
#[grm(from = "User", to = "Post", ty = "AUTHORED")]
pub struct Authored {
    #[serde(skip)]
    pub id: AuthoredId,
    pub year: u64,
}

