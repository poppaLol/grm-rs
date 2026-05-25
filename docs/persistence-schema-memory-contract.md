# Persistence And Schema Memory Contract

Status: Draft

Date: 2026-05-25

This document records decisions about how GRM should persist graph data,
runtime schema, schema memory, and durable operational metadata across CLI,
Python, MCP, Rust library, and future service surfaces.

It builds on:

- [ADR 0001: Separate Graph Data From Schema Memory](adr/0001-graph-data-and-schema-memory.md)
- [ADR 0004: Frame GRM As A Structured Operational Memory Layer](adr/0004-structured-operational-memory-layer.md)
- [ADR 0005: Use Graph Workspaces And Durable Envelopes](adr/0005-graph-workspace-and-durable-envelope.md)
- [Durability Testing Note](durability-testing.md)
- [Import / Export](import-export.md)

## Decision: Canonical Persisted Unit

The canonical persisted unit is a **graph workspace**.

A graph workspace is the resumable state a user or service opens, mutates, and
later resumes. It must be meaningful across CLI, Python, MCP, Rust library, and
future service usage.

For now, one workspace contains exactly one logical graph space. Nodes and edges
inside that workspace share one runtime schema and one operational history. A
future service may host multiple workspaces, but multi-workspace service
management is not part of the current persistence contract.

## Workspace Contents

A graph workspace should resume with:

- user graph data: nodes, edges, properties, labels, relationship types, and
  backend identity state
- runtime schema: declared node models, edge models, fields, required flags,
  ID field names, endpoint constraints, and whether each model/link is declared
  or inferred
- schema memory metadata: orientation data that explains the intended graph
  shape even when parts of the graph are empty
- durable operational history: transactions or durable deltas applied to the
  workspace, plus checkpoint/recovery metadata
- derived metadata policy: enough durable policy to rebuild backend-maintained
  derived state such as indexes, while treating derived index contents as
  rebuildable rather than source-of-truth data

The workspace should not require a caller to rediscover schema by inspecting
existing data. A model with zero nodes can still be part of the workspace if
schema memory declares it.

## Workspace State Categories

The workspace contains several kinds of state with different durability
responsibilities.

### Durable Source Of Truth

These must be recoverable for the workspace to resume correctly:

- graph data records: nodes, edges, properties, labels, relationship types, and
  identity counters or equivalent backend identity metadata
- runtime schema definitions: node models, edge models, fields, required flags,
  ID field names, endpoint constraints, schema version markers, and
  declaration/inference provenance for each model/link
- schema memory metadata: descriptions, recall affordances, intended-but-empty
  model/link concepts, and future compatibility or migration metadata
- user-defined index declarations: manual index definitions or durable indexing
  policy chosen by the user or service
- durable operation log: committed workspace transactions/deltas after the last
  checkpoint
- checkpoint metadata: the checkpoint identifier, format/version information,
  durable high-water marks, and recovery metadata needed to replay later log
  records exactly once
- compaction epoch metadata: the epoch or generation boundary that says which
  checkpoint/log range is authoritative after log compaction

### Rebuildable Derived State

These may be persisted as an optimization, but they are not the workspace source
of truth:

- backend-maintained system index contents
- automatically selected acceleration structures
- cached adjacency, matrix, projection, or planner state
- explain/profile observations used to inform future acceleration

After recovery, GRM should be able to rebuild derived state from graph data,
runtime schema, schema memory metadata, and durable index/acceleration policy.

### Declared And Inferred Schema

Runtime schema entries should distinguish declared schema from inferred schema
at the model/link level.

- **Declared schema** is authored by a developer, agent, operator, migration, or
  service workflow through GRM surfaces such as Rust library calls, CLI commands,
  Python helpers, MCP tools, or future service DTOs. Declared schema is the
  source of truth for intended workspace structure and validation.
- **Inferred schema** is derived from observed graph data, usually when GRM
  imports or attaches to graph structure that originated outside the GRM
  ecosystem. Inferred schema is evidence about current data shape, not automatic
  authority to redefine the workspace contract.

The declared/inferred marker belongs on node models and edge/link models, not
only on the workspace as a whole. A workspace may contain a mix of declared and
inferred models during import, recovery, or reconciliation.

Normal typed writes should validate against declared schema and reject
violations unless the caller is explicitly running an import, reconciliation, or
schema evolution operation. Inferred schema can seed proposals, warnings, drift
reports, and onboarding flows, but promotion from inferred to declared should be
an explicit durable operation.

### Index Policy

Index state has two layers:

- **Durable index policy**: user-defined indexes, manual indexing choices, and
  future acceleration policy that should survive workspace resume.
- **Derived index contents**: concrete backend-maintained index entries, caches,
  adjacency structures, or accelerator materializations that can be rebuilt.

Manual indexes belong in durable workspace metadata. Automatic indexes may leave
durable policy or planner evidence if GRM needs to explain or reproduce why they
exist, but their physical contents should remain rebuildable.

### Log And Compaction Epochs

The workspace log records committed operational deltas. A checkpoint represents
a compacted state boundary. A compaction epoch identifies the authoritative
relationship between checkpoints and logs after compaction.

At a minimum, recovery needs to know:

- which checkpoint is the base state
- which log records after that checkpoint must be replayed
- which older logs or checkpoints have been superseded
- whether a final log record was incomplete and should be ignored or repaired

Current implementation does not yet define a rich compaction epoch model. This
document reserves the concept so future WAL/checkpoint work has a clear place to
record it.

## Surface Expectations

All primary surfaces should open and resume the same conceptual workspace:

- CLI `session.save` / `session.load` are workspace persistence operations.
- Python save/load helpers should preserve the same workspace state, including
  schema.
- MCP should not be the only surface with durable schema memory. Its current
  schema-template behavior is a useful transitional path, not the final product
  boundary.
- Rust library callers should have access to the same workspace persistence
  semantics without depending on CLI command text.
- Future service mode should expose workspace open/resume semantics through
  typed service/admin requests, not server-local file paths in the public
  client contract.

## Workspace Open And Load Semantics

All surfaces should converge on a workspace open/load operation.

At minimum, opening a workspace locates the graph data and establishes a runtime
schema for that graph. There are two important load modes:

- **Closed-loop load**: the workspace was persisted by GRM. Graph data, runtime
  schema, schema memory metadata, durable logs/checkpoints, and durable policy
  are loaded together. Declared schema remains declared.
- **Open-loop load**: the graph data came from outside the GRM ecosystem, or GRM
  attaches to an external graph without a trusted GRM workspace envelope. GRM may
  infer model/link schema from observed data, mark that schema as inferred, and
  use it for orientation, validation proposals, and drift reports rather than as
  an automatic declared contract.

After load, GRM should perform a quick consistency pass. The first version can
be checksum- or summary-oriented rather than a full scan. Its job is to detect
whether graph data appears to have drifted from declared schema due to offline
changes, external writers, partial recovery, or format mismatch.

Default behavior should report drift without rejecting the workspace. A strict
mode should reject or fail the load when drift is detected. Strict mode is
appropriate for service startup, automated checks, CI, migrations, and other
contexts where silent drift would be worse than a failed open.

## Durable Workspace Envelope Direction

GRM should move toward its own durable workspace envelope rather than treating
JSON files, generic bincode snapshots, or storage-engine files as the product
format.

The envelope is not a claim that GRM is becoming a file database. It is the
portable operational-memory boundary for a graph workspace: the data needed to
reopen a workspace with the same runtime schema, schema memory, durable
operational history, index policy, and recovery boundaries.

Direction:

- Service/local durable workspace storage should use a GRM-owned binary
  workspace envelope once the envelope is designed.
- JSON and bincode representations may remain useful for in-memory sessions,
  debugging, tests, fixtures, and transitional local tooling.
- Interchange JSON remains separate from workspace persistence. It is for moving
  graph data between tools, not for representing the whole durable workspace
  envelope.

The binary encoding is not specified in this decision. The decision is that the
authoritative product persistence boundary should eventually be a GRM workspace
envelope that can carry graph data, schema, schema memory metadata, durable
history, index policy, and recovery boundaries together.

## In-Memory And Backend Modes

In-memory is a first-class deployment mode for embedded and local surfaces:
CLI, Python, MCP, and Rust library callers may all use an in-memory workspace
backend.

A closed-loop, reloadable in-memory workspace with autocommit can still present
the same SOML view as a service-hosted workspace: typed runtime schema,
schema-memory orientation, durable deltas/checkpoints, explainable state
resolution, and adapter-independent runtime semantics. The difference is the
durability and coordination class, not the operational-memory model.

As it matures, in-memory mode should support autocommit and reloadable workspace
state by default for user-facing workflows. It remains useful for tests,
scripts, local utilities, embedded applications, and local agent memory where
the user accepts that local file loss or corruption can effectively reset the
workspace.

Service mode is different. A service may use in-memory execution internally, but
the product service should not rely on purely ephemeral in-memory state as its
durable storage story, and it should not claim multi-writer or hosted durability
from a local autocommit deployment mode.

All surfaces should also be able to connect to Neo4j for developer visibility
and inspection. When CLI, Python, or MCP use Neo4j as the graph-data backend,
GRM may keep runtime schema/schema memory in a GRM-owned in-memory or workspace
metadata store instead of writing GRM schema metadata into the user's Neo4j graph
by default. This keeps Neo4j graph data inspectable without muddying it with GRM
metadata unless a later design explicitly opts into backend-resident metadata.

## Allowed Claims For Now

Current durability and service claims must stay conservative:

- single-writer local filesystem durability only where tested
- no multi-writer service claims
- no hosted durability claim
- no full recovery guarantee beyond tested behavior
- durable recovery depends on later roadmap work around workspace format, WAL,
  checkpoints, replay, and compaction epochs

## Schema Evolution

Schema can change over time, but schema drift is not an ordinary graph write.

If a schema change would make existing graph data invalid, GRM should require a
special schema-evolution or migration path rather than silently accepting the
drift. That future path should be explicit about validation, compatibility,
backfill/default behavior, rollback, and durable records.

For now, schema mutations should remain durable workspace operations, and
schema/data consistency should be preserved by write-time validation.

## Non-Goals

- No multi-workspace service implementation in this decision.
- No multi-tenant workspace isolation model yet.
- No distributed or multi-writer durability claim.
- No final binary workspace envelope layout yet.
- No schema migration engine in this decision.
- No claim that MCP Neo4j schema-template files are the final workspace storage
  design.

## Open Questions

- What is the exact GRM binary workspace envelope layout?
- How should workspace transactions/deltas be represented for append, replay,
  compaction, and attestation?
- When, if ever, should Neo4j-backed workspaces store GRM schema memory inside
  Neo4j instead of a GRM-owned metadata store?
- How should future service mode distinguish "open workspace" from "create
  workspace" and "attach to existing workspace"?
