# GRM SOML Schema & Catalog Architecture Direction

Status: exploratory architecture note

## Overview

GRM distinguishes between:

1. **User/Data Graph**

   * Domain entities and relationships
   * The primary operational graph data

2. **Schema/Catalog Graph**

   * Definitions describing graph structure and semantics
   * Node types
   * Relationship types
   * Property definitions
   * Constraints
   * Affordances
   * Runtime semantics

3. **Compiled Runtime Schema**

   * An in-memory representation optimised for:

     * validation
     * planning
     * RPC exposure
     * MCP integration
     * traversal semantics
     * capability resolution

The important architectural distinction is:

> The runtime schema cache is disposable.
> The graph-native catalog is authoritative.
> The catalog authority belongs to the graph workspace / durable workspace
> envelope, not to an adapter-local cache.

---

# Background

Traditional relational databases already separate:

* **user data**
* **schema metadata**

even though both are often represented using the same underlying storage primitives.

## PostgreSQL

Postgres stores schema metadata in internal catalog tables such as:

* `pg_class`
* `pg_attribute`
* `pg_type`
* `pg_constraint`

These are effectively “tables about tables”.

## MS SQL Server

MS SQL Server exposes similar metadata structures through:

* `sys.tables`
* `sys.columns`
* `sys.types`
* `sys.indexes`

Again, the database stores metadata describing the database itself.

---

# GRM Equivalent

GRM follows a similar conceptual model, but graph-native.

Instead of:

```text
tables describing tables
```

GRM uses:

```text
graphs describing graphs
```

This is natural because the core runtime and traversal machinery are already graph-oriented.

Example conceptual structure:

```text
Catalog Graph
├── (:NodeType { label: "Person" })
├── (:RelType { type: "WORKS_FOR" })
├── (:Property { name: "email", type: "string" })
└── (:Constraint { kind: "required" })
```

This catalog graph is distinct from ordinary domain/user data.

---

# Why Separate Catalog and User Data?

Keeping catalog/schema information logically distinct from user data provides:

* safer migrations
* clearer authority boundaries
* planner stability
* backend-independent semantics
* easier recovery
* schema versioning
* RPC compatibility guarantees
* future capability-based access control
* cleaner MCP projection
* support for compiled/runtime optimisation

It also prevents accidental traversal leakage between:

```text
domain graph data
```

and:

```text
runtime/schema internals
```

---

# Current Backend Situation

GRM currently supports multiple backend targets:

## 1. InMemory Backend

In-memory can be used in two modes:

* ephemeral scratch/test mode
* closed-loop local workspace mode with autocommit and reload

In scratch mode, both:

* user graph
* catalog/schema graph

may exist purely in memory, and persistence is not expected.

In closed-loop local workspace mode, the in-memory execution state can still
present the same SOML view as a service-hosted workspace when it is backed by a
durable workspace envelope: graph data, declared catalog/schema memory, durable
deltas/checkpoints, and rebuildable derived state are reopened together. The
difference is durability and coordination class. Local file loss or corruption
can still reset memory unless tested recovery behavior proves otherwise, and
local autocommit does not imply hosted durability, multi-writer safety, service
authorization, audit, observability, or managed lifecycle.

---

## 2. Neo4j Backend

When development began I wanted a quick and easy way to show that the graph concepts translated, and to be able to query the data in Cypher for validation. However, as a backend, Neo4j is currently an outlier:

* it is a third-party persistence engine
* it already contains its own schema/index concepts
* GRM-specific semantics extend beyond native Neo4j capabilities
* whereas, Neo4j is a more mature product

Examples of GRM-specific semantics will evolve. We may begin to provide:

* affordances
* attestation metadata
* capability semantics
* generic property typing
* sensitive
* planner hints
* operational/runtime semantics
* agent-facing schema descriptions

The question therefore becomes:

> Should GRM schema metadata be persisted inside Neo4j itself?

Both options are considered valid.

### Option A — Persist Catalog Inside Neo4j

Schema graph exists within Neo4j under reserved namespaces/labels.

Example:

```text
(:__grm_schema_NodeType)
(:__grm_schema_Property)
```

Advantages:

* unified persistence
* transactional proximity
* simpler backup alignment

Disadvantages:

* backend coupling
* catalog pollution
* potential traversal leakage
* backend-specific semantics
* operational complexity

---

### Option B — Workspace-Owned Catalog Store (Current Direction)

Schema/catalog exists outside Neo4j, owned by GRM's graph workspace / durable
workspace envelope.

Neo4j stores only user/domain graph data.

GRM maintains:

* persistent catalog representation
* compiled in-memory runtime schema
* MCP/RPC schema projections

Advantages:

* backend-independent semantics
* cleaner abstraction boundary
* easier portability
* consistent architecture across backends
* simpler future native backend support

Disadvantages:

* catalog/data sync considerations
* recovery/version compatibility requirements

The current MCP implementation is conceptually closest to this model, although
currently the schema projection is primarily sidecar in-memory.

That means the sidecar pattern is aligned as a projection/cache shape, but the
current non-persistent sidecar schema is transitional. The durable authority
should move toward workspace-owned catalog metadata rather than remain only in
process-local memory.

---

## 3. gRPC Service-Backed Workspace

The gRPC service-backed workspace introduces long-lived remote persistence
concerns. gRPC is the service protocol and access boundary, not a storage
backend in the same sense as in-memory or Neo4j.

This makes persistent catalog authority significantly more important.

Using only transient in-memory schema creates risks:

* recovery drift
* schema mismatch
* incompatible clients
* planner inconsistency

Therefore the intended architecture is:

```text
persistent workspace catalog
    ↓
compiled runtime schema
    ↓
RPC/MCP/client exposure
```

---

# Architectural Direction

The intended long-term direction is:

```text
GRM Runtime
├── User Graph Store
├── Workspace Catalog Store
├── Schema Compiler
├── Runtime Schema Cache
└── RPC/MCP Projection Layer
```

## Core Principle

```text
Persistent workspace catalog = authority
Compiled in-memory schema = optimisation layer
```

---

# Schema Lifecycle

The expected lifecycle is:

```text
catalog mutation
    ↓
persist catalog version
    ↓
compile runtime schema
    ↓
atomically swap active schema
    ↓
notify/runtime expose
```

Clients should never directly mutate runtime schema structures.

All schema modifications should occur through catalog operations.

---

## Backend Implications

Once the catalog architecture is made real, the backend responsibilities become clearer.

### Neo4j

Neo4j remains a special case.

GRM must translate between:

- GRM’s catalog/runtime schema model
- Neo4j’s native graph persistence model
- Neo4j’s own schema/index/constraint mechanisms

This means the Neo4j backend will likely continue to require adapter-specific handling.

### gRPC Service

The gRPC service should hide catalog/data coordination behind the service boundary.

Clients should not need to know:

- where the catalog is stored
- when schema is compiled
- how runtime schema is cached
- how backend recovery is handled

The service owns this lifecycle. The client may, or may not, create a catalog
cache. The direction for my own implementations will probably choose to do so.
If a client keeps a local catalog cache, that cache is a convenience projection.
It is not the authority for the workspace unless it is explicitly promoted
through a workspace/catalog operation.

### InMemory

The in-memory backend largely does not change as an execution engine.

Because it is already memory-resident, fast schema lookup is naturally
available. There is no strong need for an explicit “recompile” step unless this
is useful for API consistency or testing.

For user-facing local workflows, however, in-memory should converge on the same
workspace open/load/autocommit semantics as CLI, Python, MCP, Rust library, and
future service clients. The catalog may be compiled in memory, but its durable
authority comes from the workspace envelope when the workflow is closed-loop and
reloadable.

### Client Layers

With this separation, multiple client surfaces can sit over the same runtime model:

- MCP
- Python
- CLI
- Rust
- future TypeScript/JavaScript
- future C#

These clients should all interact with GRM through stable schema/catalog/data APIs rather than backend-specific details. Each may require specific environment variables to enable this, but we can attempt to make these as common as possible for ease of use, with sensible defaults per target architecture where these are missing.

# Future Possibilities

This architecture enables future support for:

* tenant-specific schemas
* dynamic client-defined schemas
* schema version negotiation
* schema migrations
* attested schemas
* capability-aware schemas
* agent-operable schema discovery
* graph-native operational memory
* distributed schema sync
* native GRM storage engines

---

# Conclusion

The current direction is to treat schema as:

* graph-native
* authoritative
* persistable
* backend-independent
* workspace-owned

while maintaining:

* high-performance in-memory compiled schema projections and caches
* flexible backend support
* RPC/MCP friendliness
* future runtime extensibility

The existing sidecar in-memory schema approach is therefore not merely a
temporary trick. It is aligned with the long-term architecture when understood
as a projection/cache pattern. It is still transitional when it is the only
place schema authority lives; durable authority should belong to the graph
workspace catalog.
