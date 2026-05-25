# Future Product Roadmap

This document captures the longer-term product direction for `grm-rs`.

It is intentionally broader than the near-term CLI roadmap. The aim here is to describe where the product could logically go as a graph workspace, runtime schema system, typed Rust library, and Python-friendly analysis tool.

## Positioning

`grm-rs` is already growing into more than one thing:

- a typed Rust graph-relational model library
- an interactive graph session CLI
- a runtime schema playground for local graph work
- a first-pass Python integration surface
- an MCP server surface for agent workflows

The long-term opportunity is not just "more commands". It is a coherent graph workbench that supports:

- application development in Rust
- local graph exploration and analysis
- repeatable scripted scenarios
- machine-friendly import/export
- Python-driven experimentation
- agent-driven graph construction and analysis through MCP
- eventual multi-process or shared workflows

## Long-Term Product Themes

### 1. Python And MCP As First-Class Surfaces

The Python extension and MCP server should grow as peer surfaces rather than
separate experiments.

Long-term direction:

- shared semantics for schema definition, CRUD, traversal, query execution,
  import/export, and batch operations
- Python APIs that feel natural for analysis and scripting
- MCP tools that are equally capable for agent-driven workflows
- common documentation and demo scenarios that show the same graph tasks through
  Python, CLI, and MCP

This matters more than adding many language integrations early. Other surfaces,
such as a C# LINQ provider, can stay future possibilities until there is a clear
workflow that justifies them.

### 2. Real Backend Support

The completed Cypher translator spike validated the first backend portability
path. The next product-level step is a real backend, not only string generation.

Long-term direction:

- a live Neo4j backend that executes translated `GraphQuery` values
- shared query and repository tests across in-memory and Cypher backends
- a backend contract that makes rows, errors, transactions, IDs, and capability
  reporting explicit
- enough Cypher compliance to support serious graph workloads rather than only
  smoke tests

### 3. Durable Local Operational Workspace

The in-memory backend should remain useful for tests and local workflows, but a
closed-loop, autocommit, reloadable workspace can also serve as a local
operational memory deployment mode without requiring a service. The product
target is not a standalone file database; it is a durable workspace envelope
that reopens typed operational memory with schema, deltas, recovery metadata,
and rebuildable derived state.

Long-term direction:

- indexed transaction overlays/read-views without whole-store copies on common
  paths
- append-friendly durability and recovery decisions after transaction deltas are
  stable
- compaction, repair, and operational tooling for local workspace envelopes
- clear durability claims grounded in tested failure modes

### 4. Demo-Driven Product Proof

The project needs multiple concrete use cases that prove the surfaces are
coherent.

Long-term direction:

- ORM-like typed Rust demos using repositories and model derives
- query-like demos that show `GraphQuery`, traversal, filtering, and backend
  portability
- Python analysis demos over the same scenarios
- MCP demos that construct, query, and update similar graphs through agent tools
- fixtures that double as tests, docs, and onboarding examples

### 5. Durable Graph Workspace

The current session model already points toward a real workspace product: a
resumable operational memory context rather than a saved graph file.

Long-term direction:

- robust persistence and recovery
- safer autocommit behavior
- better session coordination across processes
- durable local graph work that feels trustworthy

This is the foundation for almost every later capability.

### 6. Runtime Schema As A First-Class Engine Concept

Today, runtime schema exists, but it still leans heavily on the CLI/session layer.

Long-term direction:

- move runtime schema deeper into shared core abstractions
- make schema usable consistently across CLI, Rust, Python, and persistence flows
- treat models, links, and validation rules as durable product concepts rather than only interactive session metadata

### 7. Multi-Surface Product

The product naturally wants to serve several kinds of users.

Long-term direction:

- Rust library surface for typed application development
- CLI workspace for interactive graph work
- Python surface for analysis and automation
- MCP surface for agents and tool-using assistants
- possible future service mode for shared or remote access

This should become a deliberate product strategy rather than an accidental collection of entrypoints.

### 8. Service-Hostable Runtime Contract

GRM now has the first concrete proof that the future service boundary can drive
existing runtime behavior without introducing a daemon or a second semantics
path.

Current progress:

- `grm-service-api` contains codegen-checked protobuf source files and generated
  Rust DTOs for typed service operation families.
- Generated protobuf schema and batch requests can be converted into typed
  runtime requests and executed in-process through `SessionState::execute_runtime`.
- Runtime dispatcher batch execution reuses the existing batch implementation
  and preserves grouped durable operation metadata.

Long-term direction:

- keep service DTOs mapped to shared runtime behavior rather than adapter or
  backend-specific shortcuts
- add service context, authorization, limits, audit, and transport only after
  the typed runtime boundary is stable enough to review
- introduce SOML concepts such as session context, durable deltas, projections,
  and attestations only when the runtime can make and test those claims
- keep unsupported surfaces explicit instead of filling gaps with textual query
  language or service-only behavior

## Product Directions Not Yet Fully Represented

### Schema Migration And Versioning

As soon as runtime schema is persisted and shared, schema evolution becomes a core product need.

Future possibilities:

- versioned schema definitions
- migration commands for model and link changes
- rename and field-mapping support
- default/backfill strategies for newly required fields
- preflight validation before a migration is applied

This would connect the interactive session model with more serious persisted usage.

### Constraints And Integrity Rules

The current surface supports required fields and field types, but a richer rule system is a natural next step.

Future possibilities:

- unique fields
- cardinality constraints
- link endpoint constraints
- delete behavior rules such as restrict or cascade
- stronger validation reporting

This would make runtime schema significantly more valuable and more trustworthy.

### Named Queries And Reusable Views

The query surface is growing, but it is still fully ad hoc.

Future possibilities:

- saved named queries
- reusable graph views
- parameterized query templates
- sharable analysis commands across CLI and Python

This would help the product support repeated workflows rather than only one-off inspection.

### Explain, Profile, And Debugging Tooling

As traversal and backend support get richer, users will need better introspection into query behavior.

Future possibilities:

- richer `explain` output for query planning and execution shape
- profiling information for query cost and result shaping
- clearer debugging for traversal and filtering behavior
- import/export validation diagnostics

Current explain/profile output already exposes logical plan steps plus
machine-readable access-path metadata for GRM's automatic system indexes. Those
indexes are derived backend acceleration structures, not user-defined indexes or
durable source-of-truth data. User-defined indexes, constraints, and optimizer
reordering remain future work.

This would build on the existing kernel/query direction and improve trust in the system.

### Visual Schema Exploration

The product already has graph-shaped data output, but schema visualization is still missing.

Future possibilities:

- schema graph rendering for models and links
- model/link browsing with richer summaries
- graph-oriented schema diagrams in CLI or exported form

This would make runtime-defined graphs easier to understand and demo.

### Data Quality And Linting

Once sessions become more durable and import/export lands, data quality tooling becomes a natural product extension.

Future possibilities:

- graph lint commands
- orphan and inconsistency detection
- suspicious type/value drift checks
- unused model or link detection
- health reports before export or migration

This would make the tool useful not just for storing graph data, but for maintaining it well.

### Notebook And Dataframe Interoperability

The Python surface suggests a broader analysis story than the current docs describe.

Future possibilities:

- pandas or polars-friendly result export
- graph-to-dataframe helpers
- notebook-oriented exploration flows
- NetworkX-style interoperability where it makes sense

This would make `grm-rs` more compelling as a local graph analysis tool.

### PowerShell And Other Language Extensions

The product already spans Rust, CLI, Python, and MCP, so additional language integrations are a logical extension where they unlock real workflows. They should not outrank Python/MCP parity, real backend support, resilient local operations, or demo coverage.

Future possibilities:

- a PowerShell module for accessing graph and session logic from scripts and automation
- a C# LINQ provider over the portable graph query IR if .NET workflows become a concrete product need
- admin-friendly command wrappers for local graph inspection and maintenance
- language extensions added deliberately where they open up strong user workflows
- shared core abstractions so new bindings do not reimplement behavior inconsistently

This would be especially useful for scripting, automation, and environments where PowerShell is already a primary operator tool.

### Local Service Mode

The roadmap currently centers on CLI and library usage, but a lightweight service mode is a plausible long-term evolution.

Future possibilities:

- local API process over a stable session
- multi-process access without requiring direct embedding
- a cleaner foundation for pubsub and subscriptions
- better automation and tool integration

This could become the bridge between a local developer tool and a shared graph workspace.

### Clustering And High-Availability Deployment

If the product grows beyond a single local process or single-machine workflow, clustering becomes a logical longer-term direction.

Future possibilities:

- support for deployment patterns that rely on OS-level clustering, especially on Linux
- clearer behavior for shared storage, failover, and process coordination
- optional built-in clustering or replication semantics if the product grows far enough in that direction
- explicit tradeoffs between simple single-node operation and clustered high-availability setups

This would matter most if `grm-rs` evolves from a local graph workspace into a more durable shared service.

### Scenario Fixtures And Repeatable Graph Stories

The existing `.grm` script flow is already close to a fixtures system.

Future possibilities:

- first-class demo and test fixtures
- scenario loading and reset flows
- expected-output snapshots for graph scenarios
- curated example packs for onboarding and evaluation

This would strengthen the product for testing, demos, tutorials, and agent workflows.

### Encryption And Data Protection

As the product grows into a more durable local workspace, data protection becomes a meaningful product capability rather than just an implementation detail.

Future possibilities:

- encryption for persisted database or session files at rest
- protected save/load flows for sensitive local graph data
- property-level encryption for selected fields
- schema-aware handling for encrypted properties so callers know what is protected
- clear boundaries between searchable fields and encrypted fields

This would help the product support more sensitive datasets without giving up its local-workspace strengths.

## Long-Term Database Possibilities

These are intentionally framed as far-future possibilities rather than planned near-term commitments.

If `grm-rs` continues evolving toward a fuller database platform, the product could eventually grow into more classic database capabilities such as:

- backup, restore, and possibly point-in-time recovery
- replication for resilience, read scaling, or disaster recovery
- indexing, including unique and composite indexes
- richer query planning and optimization
- access control, authentication, and audit trails
- entity history, temporal queries, and time-aware graph views
- triggers, rules, and event-driven automation
- full-text search and other specialized indexing modes
- materialized views, projections, or cached graph read models
- sharding, partitioning, or multi-tenant isolation if scale requires it
- observability tooling such as metrics, health reporting, and slow-query diagnostics
- storage lifecycle features such as compaction, corruption detection, and repair tooling
- extension or plugin surfaces for validators, importers, exporters, and custom logic

The right framing for these is not "the product must become all of these".

It is that a graph system with persistence, schema, querying, import/export, security, and multi-process coordination could naturally grow in some of these directions over time if real usage demands it.

## Import / Export Direction

Import/export belongs in the long-term product story, but it should remain distinct from local workspace persistence.

### Separation Of Concerns

Use different mechanisms for different jobs:

- `.grm` scripts for human-authored setup, examples, demos, and tests
- `session.save` / `session.load` for restoring a local workspace snapshot
- `session.import` / `session.export` for machine-friendly graph interchange

Even if the underlying representations overlap, the user-facing semantics should stay separate.

### Likely Format Roles

- `JSON` for structured full-session or full-graph interchange
- `JSONL` for streaming-oriented bulk import/export and pipeline compatibility
- binary for compact, high-speed local persistence and snapshot restore

### Implementation Bias

Import/export should be bulk-oriented rather than replaying one command at a time.

That means:

- parse in batches
- validate in batches
- create nodes in batches
- create edges in batches
- avoid per-object transaction overhead where possible

### Why This Matters

This keeps `.grm` useful for authored workflows while allowing import/export to grow into a proper interchange surface for:

- large graph movement
- downstream analysis
- external tool interoperability
- durable bulk data loading

## Suggested Horizon View

### Nearer Long-Term

- Python and MCP parity over schema, CRUD, traversal, import/export, and batch operations
- service API DTO mapping over shared runtime behavior, then minimum SOML
  service additions for session context, durable deltas, projections, and
  attestation evidence
- minimal live Neo4j backend, then broader Cypher-compliant backend support
- indexed transaction overlay/read-view for the local backend
- demo scenarios covering typed Rust, query-style usage, Python, and MCP
- runtime schema refactor
- backend-neutral identity model
- import/export command family
- session coordination semantics

### Mid Long-Term

- Redis-like resilient local backend operations: recovery, compaction, repair, and durability tooling
- schema migrations
- constraints and integrity rules
- named queries and reusable views
- data quality and linting
- richer graph and schema visualization
- encryption and protected persistence options

### Farther Long-Term

- local service mode
- clustering and high-availability deployment support
- pubsub and live subscriptions
- notebook and dataframe workflows
- PowerShell and other language extensions where they unlock strong workflows
- selected database-platform capabilities such as replication, indexing, audit/history, and advanced recovery
- import or inference from existing persisted backends
- optional code generation from discovered schema

## Working Principle

The product should keep its current strengths:

- strong typing where typing matters
- approachable interactive workflows
- readable human-authored scripts
- explicit, inspectable behavior

But over time it can grow into something larger:

a graph workspace that is equally useful for developers, analysts, scripts, and automation.
