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
use serde::{Deserialize, Serialize};
use serde_json::Value;

typed_id!(UserId);
typed_id!(PostId);
typed_id!(AuthoredId);

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

Projection v1 + Typed Kernel Results (GraphQuery → QueryResult)

This addendum captures the latest work: projection/return control is now real, and query execution returns typed kernel values keyed by VarId (no stringly "n" conventions, no var_key).

What changed recently
Projection v1: explicit return target

GraphQuery already had a singular ret: Return. We extended the DSL/compiler so users can choose what node is returned:

default: return the root node

new: .return_end() returns the end node of the traversal chain

This is achieved by compiling Query<M> to GraphQuery with:

ret = Return::Node(<root_var>) by default

ret = Return::Node(<end_var>) when .return_end() is used

In-memory executor semantics updated (correct + explicit)

The in-memory executor now:

Seeds from the real root NodeMatch (first MatchClause::Node), independent of what is returned.

Applies chained HopMatch traversal using GraphTx::{outgoing,incoming,both} with correct wildcard semantics when rel_type == None.

Collects returned IDs based on GraphQuery.ret:

returning root var ⇒ return Binding.root

returning end var ⇒ return Binding.cur

Emits QueryRow results keyed by VarId, not String.

Typed kernel result shape (no JSON blob keys)

QueryRow now carries strongly-shaped graph facts (kernel-level), not ad-hoc JSON with magic keys:

QueryRow.values: BTreeMap<VarId, Value>

Value::Node(NodeValue { id, labels, props })

This makes repo decoding and future Neo4j mapping much safer and more direct.

## 🚧 Roadmap (Short Term)

Planned next steps:

* Adjacency indexes for performance
* Persisted backends
* Neo4j backend
* CLI for interactive graph inspection
