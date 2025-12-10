# grm-rs — Graph Relational Model for Rust

**grm-rs** is a strongly typed Graph Relational Model framework for Rust.

It provides:

- 🧩 **Typed node and relationship models**
- 🪪 **Typed ID newtypes (`UserId`, `PostId`, …)**
- 🔧 **Derive macros (`#[derive(NodeModel)]`, `#[derive(RelModel)]`)**
- 🗄️ **Repository layer for CRUD + traversal**
- 🧪 **In-memory graph backend for testing**
- 🔁 **Transaction support (commit + rollback)**
- 🚀 **A path toward a real Neo4j backend**

The goal is to give Rust developers a type-safe, ergonomic OGM/ORM-like interface for graph data.

Over time as the project gets legs, we also want to wrap other engines, as well as give a command line interface.

The CLI will allow users, as well as agentic AI, to store related data entities either in memory for point in time immediate analysis, or persisted for future query.

---

## ✨ Features

### 🧬 Typed Entities
Define your graph schema using Rust structs:

```rust
use grm_rs::{NodeModel, RelModel, typed_id};
use serde::{Serialize, Deserialize};

typed_id!(UserId);
typed_id!(PostId);
typed_id!(AuthoredId);

#[derive(Serialize, Deserialize, NodeModel)]
struct User {
    id: UserId,
    name: String,
}

#[derive(Serialize, Deserialize, NodeModel)]
struct Post {
    id: PostId,
    title: String,
}

#[derive(Serialize, Deserialize, RelModel)]
#[grm(from = "User", to = "Post", ty = "AUTHORED")]
struct Authored {
    id: AuthoredId,
    year: i32,
}
