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

## Projection v1 + Typed Kernel Results  
**(GraphQuery → QueryResult)**

This addendum captures the latest work: **projection / return control is now real**, and query execution now returns **typed kernel values keyed by `VarId`** — no stringly `"n"` conventions and no `var_key`.

---

## What Changed Recently

### 🚀 Projection v1: Explicit Return Target

`GraphQuery` already supported a singular `ret: Return`. The DSL and compiler were extended so users can now **explicitly choose which node is returned**.

#### Behavior

- **Default**  
  Returns the **root node** of the query.

- **New**  
  `.return_end()` returns the **end node of the traversal chain**.

#### Compilation Semantics

When compiling `Query<M>` into `GraphQuery`:

- **Default**
  ```rust
  ret = Return::Node(<root_var>)

* **With `.return_end()`**

  ```rust
  ret = Return::Node(<end_var>)
  ```

This preserves a single return target while allowing precise projection control. This needs more work to allow selection of any available part of the query path, but lays the foundation for arbitrary projection.

---

### 🧠 In-Memory Executor Semantics (Correct + Explicit)

The in-memory executor was updated to fully align with the new projection model.

#### Execution Flow

1. **Root Seeding**

   * Always seeds from the *real root* `NodeMatch`
   * Determined by the first `MatchClause::Node`
   * Independent of what is ultimately returned

2. **Traversal**

   * Applies chained `HopMatch` traversal
   * Uses:

     ```rust
     GraphTx::{outgoing, incoming, both}
     ```
   * Correct wildcard semantics when:

     ```rust
     rel_type == None
     ```

3. **Result Collection**

   * Based on `GraphQuery.ret`:

     * Returning **root var** → `Binding.root`
     * Returning **end var** → `Binding.cur`

---

### 🧩 Typed Kernel Result Shape (No JSON Blob Keys)

Query results are now **strongly typed at the kernel level**, removing ad-hoc JSON blobs and magic string keys.

#### New Result Model

```rust
QueryRow {
  values: BTreeMap<VarId, Value>
}
```

#### Value Shape

```rust
Value::Node(NodeValue {
  id,
  labels,
  props
})
```

#### Key Improvements

* Results keyed by **`VarId`**, not `String`
* No `"n"`, `"m"`, or other stringly conventions
* Strongly shaped graph facts
* Safer repository decoding
* Direct and future-proof Neo4j mapping

---

## Summary

* ✅ Projection v1 introduces **explicit return control**
* ✅ Executor behavior is now **correct, explicit, and decoupled**
* ✅ Query results are **typed, structured, and kernel-safe**
* 🏋 No more magic strings or loosely-shaped JSON blobs

This lays a solid foundation for richer projections, safer execution, and cleaner integrations going forward.

## 🚧 Roadmap (Short Term)

Planned next steps:

* Adjacency indexes for performance
* Persisted backends
* Neo4j backend
* CLI for interactive graph inspection
