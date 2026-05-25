# ADR 0005: Use Graph Workspaces And Durable Envelopes

Status: Accepted

Date: 2026-05-25

## Context

GRM is moving toward a Structured Operational Memory Layer for applications and
agents. That direction requires more than graph data persistence. A resumed
memory context must know the graph data, the runtime schema, schema-memory
metadata, durable operational history, recovery boundaries, and durable policy
needed to rebuild derived state.

Current GRM surfaces are not yet uniform:

- CLI and Python can save/load local sessions.
- MCP can recover schema memory through transitional schema-template files.
- Neo4j can store user graph data while GRM schema memory may live outside the
  user's Neo4j graph.
- Future service work needs typed open/resume semantics without exposing
  server-local file paths as the public contract.

Without a clear persisted unit, work can drift toward unrelated concepts:
session files, stores, databases, collections, graph snapshots, or backend
storage formats. That would weaken the SOML direction by making storage
mechanics look like the product surface.

## Decision

The canonical persisted unit is a **graph workspace**.

A graph workspace is one resumable operational memory context. For now, one
workspace contains one logical graph space with one runtime schema and one
operational history. A future service may host multiple workspaces, but
multi-workspace service management is outside this decision.

GRM should persist and resume a workspace through a **durable workspace
envelope**. The envelope is not a file-database positioning. It is the durable
operational-memory boundary that carries enough state to reopen the same typed
memory context:

- user graph data
- runtime schema
- declared/inferred schema provenance for node models and edge/link models
- schema memory metadata and orientation data
- durable operation log or deltas
- checkpoint and recovery metadata
- compaction epoch metadata when introduced
- durable index or acceleration policy

Backend-maintained index contents, caches, projections, planner state, and other
accelerators are derived state. They may be checkpointed as optimizations later,
but they must remain rebuildable from workspace source-of-truth data and durable
policy.

## Load Semantics

All primary surfaces should converge on workspace open/load semantics:

- CLI
- Python
- MCP
- Rust library
- future service/admin APIs

Closed-loop load means GRM is reopening a workspace persisted by GRM. It should
load graph data, declared runtime schema, schema memory metadata, durable
logs/checkpoints, and durable policy together. Declared schema remains declared.

Open-loop load means GRM is importing or attaching to graph data that does not
arrive with a trusted GRM workspace envelope. GRM may infer model/link schema
from observed data and mark that schema as inferred. Inferred schema is useful
for orientation, drift reports, onboarding, and proposals, but it is not
automatically the declared workspace contract.

After load, GRM should perform a consistency or drift check between declared
schema and graph data. Default behavior should report drift without rejecting the
workspace. Strict mode should reject or fail the load when drift is detected.
Strict mode is appropriate for service startup, CI, migrations, automated
checks, and other contexts where silent drift is worse than a failed open.

## In-Memory Mode

In-memory is a first-class local deployment mode for CLI, Python, MCP, and Rust
library callers.

A closed-loop, reloadable in-memory workspace with autocommit can present the
same SOML view as a service-hosted workspace: typed runtime schema, schema
memory orientation, durable deltas/checkpoints, explainable state resolution,
and adapter-independent runtime semantics.

The difference is durability and coordination class, not the operational-memory
model. Local file loss or corruption can effectively reset memory unless tested
recovery behavior proves otherwise. Local autocommit does not imply hosted
durability, multi-writer safety, service authorization, audit, observability, or
managed lifecycle.

## Format Direction

GRM should move toward its own durable workspace envelope rather than treating
JSON files, generic bincode snapshots, or storage-engine files as the product
format.

This ADR does not specify the binary encoding. It records the product and
architecture boundary: the authoritative persistence artifact should be a GRM
workspace envelope that carries operational-memory state together.

JSON and bincode representations may remain useful for in-memory sessions,
debugging, tests, fixtures, and transitional local tooling. Interchange JSON
remains separate from workspace persistence; it is for moving graph data between
tools, not for representing the whole durable workspace envelope.

## Relationship To Existing Decisions

This decision extends ADR 0001 by naming the durable scope for graph data and
schema memory: the graph workspace.

This decision reinforces ADR 0004 by keeping storage engines subordinate to
SOML semantics. The product concept is a resumable operational memory context,
not a file database, graph CRUD API, or storage engine first.

This decision also reinforces transparent acceleration work: durable index
policy may belong in the workspace envelope, while concrete acceleration
contents remain derived and rebuildable.

## Non-Goals

- No final binary workspace envelope layout in this ADR.
- No schema migration engine in this ADR.
- No multi-workspace service implementation in this ADR.
- No multi-tenant hosted service isolation model in this ADR.
- No multi-writer local filesystem claim.
- No hosted durability claim.
- No full recovery guarantee beyond tested behavior.
- No claim that MCP Neo4j schema-template files are the final workspace storage
  design.

## Consequences

Positive consequences:

- CLI, Python, MCP, Rust library, and future service work have one shared
  persistence concept to converge on.
- The SOML framing gets a concrete durable unit without turning GRM into a
  file-database product.
- Closed-loop and open-loop load semantics make schema provenance explicit.
- Local in-memory usage can mature into a credible local SOML deployment mode
  while keeping durability claims honest.
- Future WAL, checkpoint, compaction, and recovery work has a clear envelope to
  target.

Tradeoffs:

- GRM must manage schema/data consistency across workspace load, writes,
  recovery, and imports.
- Existing session save/load language will need gradual clarification.
- The durable envelope introduces design work before binary format or WAL
  layout can be considered settled.
- Agents and users need clear wording to distinguish "local autocommit
  workspace" from "hosted durable service."

## Guidance

Future persistence and service work should follow these rules:

- Prefer "workspace" for the durable operational memory unit.
- Prefer "durable workspace envelope" for the persistence boundary.
- Avoid framing the direction as GRM becoming a file database.
- Keep typed service/admin open/resume requests separate from server-local file
  paths.
- Treat declared schema as validation authority and inferred schema as evidence
  until explicitly promoted.
- Keep current durability claims to tested single-writer local behavior.
- Treat closed-loop autocommit in-memory as local SOML mode with narrower risk
  guarantees than service mode.

## Open Questions

- What is the exact binary workspace envelope layout?
- How should workspace deltas be represented for append, replay, compaction, and
  future attestation?
- What is the first strict-mode drift check that is useful without requiring a
  full graph scan?
- When, if ever, should Neo4j-backed workspaces store GRM schema memory inside
  Neo4j instead of a GRM-owned metadata store?
- How should future service mode distinguish create, open, attach, snapshot,
  restore, and migrate operations?
