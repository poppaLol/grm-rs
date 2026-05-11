# grm-rs — Graph Relational Model for Rust

**grm-rs** is a strongly typed Graph Relational Model framework for Rust.

It provides:

* 🧩 **Typed node and relationship models**
* 🪪 **Typed ID newtypes (`UserId`, `PostId`, …)**
* 🔧 **Derive macros (`#[derive(NodeModel)]`, `#[derive(RelModel)]`)**
* 🗄️ **Repository layer for CRUD + traversal**
* 🧠 **Backend-agnostic kernel IR (`GraphQuery`)**
* 🧪 **In-memory graph backend for testing**
* 🔁 **Transaction support (commit + rollback)**
* 🔍 **Typed graph traversal DSL**
* 🚀 **A path toward real graph backends (e.g. Neo4j)**

The goal is to give Rust developers a **type-safe, ergonomic OGM-style interface** for graph data, without stringly-typed queries or runtime surprises.

Over time, the project aims to support additional backends and a CLI for both humans and agentic systems, enabling graph-structured analysis either in-memory or persisted.

## Current Backend Direction

The current backend roadmap has completed the first validation pass for mapping
the backend-agnostic `GraphQuery` IR to Neo4j/Cypher. That keeps the next
in-memory work focused on an indexed transaction overlay/read-view rather than a
physical storage redesign.

The important decision: `grm-rs` is **not** currently moving directly toward
index-free adjacency. The in-memory backend remains an indexed local graph store
for now, while the project builds out portable backend contracts above it.

This branch now includes:

- an offline `GraphQuery` to Cypher translator
- named Cypher parameters as `BTreeMap<String, serde_json::Value>`
- translation tests for node matching, traversal, return shape, paging, and escaped names
- an ignored live Neo4j Bolt smoke test using `neo4rs`

The live smoke test has successfully connected to a local Neo4j instance through
`host.docker.internal:7687`. It seeds a small `User -[:AUTHORED]-> Post` graph,
executes Cypher generated from `GraphQuery`, verifies the returned node, and
cleans up the inserted data.

See [Backend Pivot: Cypher Spike Before Deeper In-Memory Storage Work](docs/backend-pivot-cypher-spike.md).

## Python bindings

A first-pass Python extension crate now lives in [`grm-python`](grm-python).

It currently targets the runtime session surface rather than the generic Rust repository APIs, which keeps the initial Python API dict/list-oriented and easier to evolve.

For the full setup and usage flow, including how to run the compiled Rust CLI as a Python-focused developer, see [docs/python-quickstart.md](docs/python-quickstart.md).

```bash
cd grm-python
maturin develop
```

```python
from grm_rs import Session

session = Session()
session.model_create(
    "User",
    "userId",
    [{"name": "name", "type": "string", "required": True}],
)
session.node_create("User", {"name": "Alice"})
```

The CLI remains the Rust binary:

```bash
cargo build --bin grm
./target/debug/grm session
```

---

## 🖥️ Interactive Session

`grm-rs` now includes a small interactive CLI for working with runtime-defined models on a fresh in-memory graph.

The entrypoint is:

```bash
cargo run --bin grm -- session
```

This starts an empty session and drops you into a prompt like:

```text
grm(session)>
```

From there you can:

* define node models with `model.define`
* define links with `link.define`
* create runtime data with `node.create` and `edge.create`
* update runtime data with `node.update` and `edge.update`
* query runtime data with `node.find` and `edge.find`
* inspect definitions with `model.list`, `model.show <name>`, `link.list`, and `link.show <name>`
* save the current graph with `session.save --json test-dbs/<name>.json` or `session.save --bin test-dbs/<name>.bin`
* import a machine-friendly graph interchange document into a new empty session with `session.import --json test-dbs/<name>.export.json`
* export a machine-friendly graph interchange document with `session.export --json test-dbs/<name>.export.json`
* keep a working session durable with `session.autocommit --json test-dbs/<name>.json` or `session.autocommit --bin test-dbs/<name>.bin`
* checkpoint the current autocommit target and clear its replay log with `session.compact`

For current limitations and planned next steps, see [docs/cli-roadmap.md](docs/cli-roadmap.md).

### Bootstrapping From A Script

You can preload models from a script and then continue working in the same session:

```bash
cargo run --bin grm -- session --script examples/session_setup.grm
```

The script is executed first, and then the CLI drops into the interactive prompt with the same in-memory state still available. This makes the typical flow:

1. bootstrap models from a script
2. enter the interactive session
3. create nodes and relationships against those runtime models

Example script commands:

```text
model.define User userId name:string:required age:int:optional
model.define Post postId title:string:required text:string:optional
link.define Authored User Post authoredId authoredOn:string:required
node.create User name="Alice Jones" age=42
node.create Post title="Hello World" text="A short welcome post about graphs."
edge.create Authored from=1 to=2 authoredOn=2026-04-10
node.update User 1 name="Alice Johnson" age=43
edge.update Authored 1 authoredOn=2026-04-12
node.find User name="Alice Johnson"
node.find User age>=21 order=age:desc limit=10
node.find User name!="Alice Jones"
edge.find Authored from=1 authoredOn>=2026-04-10 order=authoredOn:desc,to:asc
```

### Interactive Commands

Definition commands:

```text
model.define [<Name> <id_field> [field:type:required|optional ...]]
model.list
model.show <name>

link.define [<Name> <from_model> <to_model> <id_field> [field:type:required|optional ...]]
link.list
link.show <name>
```

Data commands:

```text
node.create <ModelName> [field=value ...]
node.update <ModelName> <id> [field=value ...]
node.delete <ModelName> <id>
node.find <ModelName> [field=value|field!=value|field>value|field>=value|field<value|field<=value|field~value ...] [via=<out|in|both>:<LinkName|*>:<EndModel> ...] [end.<field>=value ...] [edge.<field>=value ...] [return=root|end|edge] [order=<field>:asc[,<field>:desc ...]] [limit=<n>] [offset=<n>] [format=default|jsonl|table|graph]

edge.create <LinkName> from=<id> to=<id> [field=value ...]
edge.update <LinkName> <id> [field=value ...]
edge.delete <LinkName> <id>
edge.find <LinkName> [from=<id>] [to=<id>] [field=value|field!=value|field>value|field>=value|field<value|field<=value|field~value ...] [order=<field>:asc[,<field>:desc ...]] [limit=<n>] [offset=<n>] [format=default|jsonl|table]
```

Query examples:

```text
node.find User name="Alice Jones"
node.find User age>=21 order=age:desc,name:asc limit=10
node.find User bio~"graph databases"
edge.find Authored from=1 authoredOn>=2026-04-10 order=authoredOn:desc,to:asc
```

Traversal examples:

```text
node.find User name="Alice Jones" via=out:Authored:Post
node.find User name="Alice Jones" via=out:Accessed:Post end.title="Draft Notes"
node.find User name="Alice Jones" via=out:Accessed:Post edge.accessedOn=2026-04-20 return=edge
```

Graph output examples:

```text
node.find User name="Alice Jones" via=out:Authored:Post format=graph
node.find User name=Alice via=out:Knows:User via=out:Knows:User format=graph
```

Update examples:

```text
node.update User 1 name="Alice Johnson" age=43
node.update Post 2 text="A revised short post for the playground."
edge.update Authored 1 authoredOn=2026-04-12
edge.update Accessed 3 accessedOn=2026-04-22
```

Session commands:

```text
session.help
session.describe
session.save --json test-dbs/<name>.json
session.save --bin test-dbs/<name>.bin
session.load --json test-dbs/<name>.json
session.load --bin test-dbs/<name>.bin
session.import --json test-dbs/<name>.export.json
session.export --json test-dbs/<name>.export.json
session.compact
session.autocommit --json test-dbs/<name>.json
session.autocommit --bin test-dbs/<name>.bin
session.autocommit status
session.autocommit off
session.exit
```

### Notes

* runtime models and links are persisted with session save/load files
* `session.import --json` currently requires an empty session and raises an error if schema or graph data already exists
* `session.export --json` writes an interchange v1 draft document; see [docs/import-export.md](docs/import-export.md)
* `session.compact` requires autocommit to be enabled and rewrites the target snapshot so the replay log can be cleared
* for local scratch databases and session files, prefer keeping them under `test-dbs/` so the repo root stays tidy
* model and relationship IDs are backend-assigned; the CLI asks for the user-facing ID field name and uses the backend-reported ID type
* the current in-memory backend reports `int` IDs
* `session.autocommit` keeps a durable snapshot plus replay log for successful model/link definitions, data mutations, and `session.load`

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

Repository convenience methods roll back their internally managed transaction when
decoding or validation fails. When you open a transaction manually, you own the
choice to `commit()` or `rollback()`.

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

#### Recent work: transaction-oriented repositories

grm-rs has been moving toward a transaction-first execution model. A transaction
can be the unit of work when you use `GraphClient`, while repository facades
remain available for shorter convenience flows.

##### What changed:

Previously, repositories owned a backend and implicitly managed transaction lifecycle:

`begin_tx → do work → commit/rollback`

This made composition awkward and hid atomicity.

The current public surface supports two styles:

* `GraphClient::transaction()` when the caller wants to own the unit of work
* backend-owned `NodeRepository` / `RelRepository` facades for convenience methods
  that manage a transaction internally

Transaction-scoped helpers are available as explicit `NodeRepositoryTx` and
`RelRepositoryTx` values over an active transaction.

Example:

```rust
let mut tx = client.transaction().await?;

{
    let mut users = NodeRepositoryTx::<_, User>::new(&mut tx);
    users.create(&mut user).await?;
}

{
    let mut posts = NodeRepositoryTx::<_, Post>::new(&mut tx);
    posts.create(&mut post).await?;
}

{
    let mut authored_rels = RelRepositoryTx::<_, Authored>::new(&mut tx);
    authored_rels
        .create_between(user.id(), post.id(), &mut authored)
        .await?;
}

let q = Query::<User>::matching(
    NodePattern::<User>::new()
        .out::<Authored>()
        .to::<Post>()
);

let users: Vec<User> = tx.query(q).await?;

tx.commit().await?;
```

Transaction is the unit of work

`NodeRepositoryTx::<_, M>::new(&mut tx)` and
`RelRepositoryTx::<_, R>::new(&mut tx)` are typed, tx-scoped helpers

Within the transaction-scoped helpers, there are no hidden commits; the caller
chooses when to commit or roll back the transaction.

##### Current compatibility shape

The backend-owned repositories still exist as autocommit facades:
* They begin and commit a transaction internally
* They delegate work to transaction-scoped repository helpers
* They remain the stable convenience API while the transaction-first surface matures

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

## 🚧 Roadmap

The canonical priority list lives in [docs/cli-roadmap.md](docs/cli-roadmap.md).
Keep future ordering there so README, backend notes, and topic-specific design
docs do not drift into competing roadmaps.

Recently completed:

* Minimal live Neo4j backend prototype with Rust and Python smoke coverage
* Offline `GraphQuery` to Cypher translator with named parameters, translation tests, and an ignored live Neo4j Bolt smoke test
* Delta transaction work landed for simple in-memory write paths, reducing full-store copies for common inserts and updates
* Repository bulk insert helpers for typed nodes and relationships
* In-memory entity lookup indexes for labels, properties, relationship types, and adjacency
* Lazy node property-index rebuilds, preserving read-your-writes behavior while reducing write-time index churn
* Insert benchmark scaling and a flamegraph workflow for profiling Criterion benches
