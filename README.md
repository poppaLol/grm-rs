# grm-rs — Graph Relational Model for Rust

**grm-rs** is a strongly typed Graph Relational Model framework for Rust.

It provides:

* 🧩 **Typed node and relationship models**
* 🪪 **Typed ID newtypes (`UserId`, `PostId`, …)**
* 🔧 **Derive macros (`#[derive(NodeModel)]`, `#[derive(RelModel)]`)**
* 🗄️ **Repository layer for CRUD + traversal**
* 🧪 **In-memory graph backend for testing**
* 🔁 **Transaction support (commit + rollback)**
* 🔍 **Typed graph traversal DSL**
* 🚀 **A path toward real graph backends (e.g. Neo4j)**

The goal is to give Rust developers a **type-safe, ergonomic OGM-style interface** for graph data, without stringly-typed queries or runtime surprises.

Over time, the project aims to support additional backends and a CLI for both humans and agentic systems, enabling graph-structured analysis either in-memory or persisted.

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
```

IDs are **explicit and typed**, not hidden properties.

---

### 🗄️ Typed CRUD via Repositories

Repositories are thin, typed wrappers over the backend:

```rust
let user = user_repo.create(&mut user).await?;
let post = post_repo.create(&mut post).await?;

authored_repo.create_between(&user.id, &post.id, &mut authored).await?;
```

All CRUD is:

* strongly typed
* transactional
* backend-agnostic

---

### 🔍 Typed Traversal Queries (No Strings)

`grm-rs` provides a **typed traversal DSL** that compiles to a backend-executable IR (not pseudo-Cypher).

#### Typed relationship traversal

```rust
use grm_rs::dsl::{NodePattern, Query};

let q = Query::<User>::matching(
    NodePattern::<User>::new()
        .out::<Authored>()
        .to::<Post>()
);

let users: Vec<User> = user_repo.query(q).await?;
```

This finds `User` nodes that have an outgoing `AUTHORED` relationship to a `Post`.

---

#### Directional traversal

All directions are supported:

```rust
.out::<R>()        // outgoing
.incoming::<R>()   // incoming
.both::<R>()       // either direction
```

These are enforced at compile time via the `RelModel` definition.

---

#### Any-relationship traversal (wildcard)

For exploratory or schema-light queries, you can traverse **any relationship type**:

```rust
let q = Query::<User>::matching(
    NodePattern::<User>::new()
        .both_any()
        .to::<Post>()
);
```

This matches `User` nodes connected to `Post` nodes by **any relationship**, in either direction.

Available variants:

```rust
.out_any()
.incoming_any()
.both_any()
```

---

#### Multi-hop traversal

Traversal steps can be chained naturally:

```rust
let q = Query::<User>::matching(
    NodePattern::<User>::new()
        .out::<Authored>()
        .to::<Post>()
        .out_any()
        .to::<Post>()
);
```

Each hop is compiled into a typed, backend-executable query plan.

---

### 🧠 Query Compilation Model

The DSL (Option A) compiles into a minimal **GraphQuery IR** (Option B):

* no strings
* no runtime parsing
* no backend-specific query language

Backends execute the IR directly.

The in-memory backend executes this IR natively and is used extensively in tests.

---

### 🔁 Transactions

All operations run inside explicit transactions:

```rust
let mut tx = backend.begin_tx().await?;
tx.create_node(...).await?;
tx.commit().await?;
```

If decoding or validation fails, transactions are rolled back automatically.

---

## 🧪 In-Memory Backend

The included in-memory backend supports:

* typed CRUD
* traversal (`outgoing`, `incoming`, `both`)
* wildcard traversal (`any`)
* transactional semantics
* deterministic testing

It is intended for:

* unit tests
* experimentation
* prototyping graph logic

---

## 🚧 Roadmap (Short Term)

Planned next steps:

* Projection control (`return_end`, custom returns)
* Adjacency indexes for performance
* Persisted backends
* Neo4j backend
* CLI for interactive graph inspection
