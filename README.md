# grm-rs — Graph Relational Model for Rust

**grm-rs** is a strongly typed Graph Relational Model framework for Rust.

It provides:

* 🧩 **Typed node and relationship models**
* 🪪 **Typed ID newtypes (`UserId`, `PostId`, …)**
* 🔧 **Derive macros (`#[derive(NodeModel)]`, `#[derive(RelModel)]`)**
* 🗄️ **Repository layer for CRUD + traversal**
* 🔍 **Typed graph traversal DSL**
* 🧠 **Backend-agnostic kernel IR (`GraphQuery`)**
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

### 🔁 Transactions (Unit of Work)

Repositories are thin, typed wrappers over the backend:

All graph operations run inside explicit transactions.

The recommended entrypoint is `GraphClient`, which yields a transaction:

```rust
let client = GraphClient::new(backend);
let mut tx = client.transaction().await?;
```

Transactions support:

* atomicity
* explicit commit / rollback
* backend-agnostic execution of the kernel IR

```rust
tx.commit().await?;
```

If an error occurs, you can roll back:

```rust
tx.rollback().await?;
```

---

### 🗄️ Typed CRUD (Repositories + Transactions)

Repositories still provide typed convenience helpers for CRUD and adjacency traversal.
Query execution is now transaction-first, and repositories are expected to become
transaction-scoped over time.

Repositories are thin, typed wrappers over the backend (current shape):

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

#### ⛓️ Directional traversal

All directions are supported:

```rust
.out::<R>()        // outgoing
.incoming::<R>()   // incoming
.both::<R>()       // either direction
```

These are enforced at compile time via the `RelModel` definition.

---

#### 🔃 Any-relationship traversal (wildcard)

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

#### 🔁 Returning Relationships

In addition to returning nodes, queries can explicitly return relationships.
This is useful when the relationship itself carries meaning or data (timestamps, weights, roles, etc.) and you want to work with it as a first-class typed model.

The traversal DSL remains the same — the only difference is the projection. By calling .return_rel(), the query returns the relationship from the final hop, which can then be decoded into a strongly typed RelModel.

```rust
// Query: (User)-[AUTHORED]->(Post), return the relationship
let q = Query::<User>::matching(
    NodePattern::<User>::new()
        .filter(User::name_prop().eq("Alice"))
        .out::<Authored>()
        .to::<Post>(),
)
.return_rel();

let rels: Vec<Authored> = tx.query_rel(q).await?;
```

This avoids string-based projections and keeps both traversal and results fully type-safe.

#### 💫 Multi-hop traversal

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

Query execution produces typed kernel results, independent of any backend or storage representation.

Each result "*row*" (or entity) contains one or more values keyed by internal variable identifiers, with no reliance on positional ordering or string keys.

At the kernel level, returned values are represented as:

```rust
KernelValue::Node(NodeValue { id, labels, props })
KernelValue::Rel(RelValue { id, ty, from, to, props })
```

This makes the shape of query results explicit and predictable:

Nodes include their internal ID, labels, and property map

Relationships include their ID, type, endpoints, and property map

By standardising on these kernel types, grm-rs avoids loosely-typed JSON blobs and enables safe, structured decoding into user-defined models, while remaining compatible with multiple backends (including future Neo4j support).

#### Key Improvements

* Results keyed by **`VarId`**, not `String`
* No `"n"`, `"m"`, or other stringly conventions
* Strongly shaped graph facts
* Safer repository decoding
* Direct and future-proof Neo4j mapping

---

#### Recent work (2025/12/26) Transaction-oriented repositories (new common use of repo)

grm-rs now uses a transaction-first execution model. A transaction is the unit of work; repositories are lightweight, typed façades over an active transaction.

##### What changed:

Previously, repositories owned a backend and implicitly managed transaction lifecycle:

`begin_tx → do work → commit/rollback`

This made composition awkward and hid atomicity.

We have now refactored the system so that:

* Transactions own lifecycle (begin / commit / rollback)
* Repositories are tx-scoped and never manage lifecycle

All graph operations (node + relationship CRUD, traversal, queries) execute within a single explicit transaction

The new mental model

```rust
let mut tx = client.transaction().await?;

{
    let mut repo = tx.repo();

    repo.nodes::<User>().create(&mut user).await?;
    repo.nodes::<Post>().create(&mut post).await?;

    repo.rels::<Authored>()
        .create_between(&user.id(), &post.id(), &mut authored)
        .await?;

    let q = Query::<User>::matching(
        NodePattern::<User>::new()
            .out::<Authored>()
            .to::<Post>()
    );

    let users: Vec<User> = repo.query(q).await?;
}

tx.commit().await?;
```

Transaction is the unit of work

`tx.repo()` returns a single graph handle

`repo.nodes::<M>()` and `repo.rels::<R>()` are typed, tx-scoped repos

No hidden transactions, no implicit commits

##### Backwards compatibility

The original backend-owned repositories still exist as autocommit façades: 
* They begin and commit a transaction internally
* They delegate all logic to the new tx-scoped repositories
* They will be deprecated once users migrate to the tx-first API

This refactor unlocks:
* True atomic multi-step graph operations
* Clean composition of node + relationship work
* A consistent execution model across backends
* A solid foundation for:
  * Neo4j support
  * Other persistent backends
* Connection pooling and session management

##### Backend implications

All backends now implement the same GraphTx contract. The transaction boundary is explicit and enforced, making it straightforward to add:
* Neo4j (via execute_graph → Cypher translation)
* In-memory persistence
* Future distributed or pooled backends

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
