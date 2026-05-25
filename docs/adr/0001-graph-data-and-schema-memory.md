# ADR 0001: Separate Graph Data From Schema Memory

Status: Accepted

Date: 2026-05-20

## Context

GRM is evolving from a local runtime graph/session tool into a typed graph
memory layer that can support CLI, Python, MCP, and a future service backend.
Recent Neo4j MCP dogfooding showed that durable graph data alone is not enough
for useful recall. A fresh agent can reconnect to Neo4j and see persisted user
nodes and relationships, but it still needs orientation: which models are
intended, which fields are valid, which relationships are meaningful, and which
parts of the domain may exist even when there are currently zero instances.

This is equally important for humans arriving at a new graph-backed system. A
schemaless graph can store facts, but the declared shape of the domain tells
readers and tools how to interpret and safely extend those facts.

## Decision

Future GRM backends should treat graph memory as two related but distinct forms
of storage:

1. **Graph data**: the user's domain nodes and edges, stored with the user's
   labels, relationship types, properties, and nomenclature.
2. **Schema memory**: structured metadata describing the intended graph shape,
   including node models, edge models, fields, constraints, capabilities,
   indexes, query/recall affordances, and other orientation data.

The canonical persisted unit for schema memory is now a graph workspace. For the
current product direction, one workspace contains one logical graph space with
one runtime schema and one operational history. Future hosted/service work may
manage multiple workspaces, but multi-workspace service semantics are not part
of this ADR's current implementation scope. See
[Persistence And Schema Memory Contract](../persistence-schema-memory-contract.md).

The core decision is that schema memory is first-class architecture, not an
incidental cache inferred only from existing data.

Schema memory may be validated strictly at GRM write boundaries. This lets GRM
enforce that typed tools, SDKs, MCP calls, and future service requests create
data that matches the declared model. However, enforcement is only one part of
the value. The larger value is communication: schema memory tells agents and
humans what kind of data can exist in a domain and how future writes should be
shaped.

## Service Boundary

This decision does not change GRM's service-boundary principle: GRM should not
make a textual query language its core service contract. Schema definition,
query, explain/profile, batch patch, and admin operations should remain
structured typed requests.

Schema memory can serve a role similar to DDL, and structured recall/query
objects can serve a role similar to DQL, but they should be represented as
typed graph/JSON/protobuf objects rather than as a required T-SQL/Cypher-like
language at the service boundary.

CLI syntax, tutorials, and adapter conveniences may still provide human-friendly
text surfaces. Those are ergonomic adapters, not the canonical contract.

## Current Implementation

The current Neo4j MCP dogfood path demonstrates the split:

- Neo4j stores the user's graph data.
- `GRM_SCHEMA_TEMPLATE` points at a local GRM JSON session file used as durable
  schema memory for the runtime schema.
- MCP exposes `grm://backend/status` so agents can tell whether schema memory
  persistence is enabled and whether schema memory was recovered from an
  existing file.
- Supported schema tools and Neo4j-supported `grm_batch` schema operations can
  append schema definitions to that local schema memory file.

This implementation is intentionally transitional. It proves that schema memory
unblocks storage and recall without requiring GRM to infer schema from Neo4j
labels and properties.

## Consequences

Positive consequences:

- Agents can orient before writing instead of guessing from existing graph data.
- Humans can inspect the intended domain shape even when the graph is sparse.
- Empty-but-valid model types can exist. A model with zero nodes is still part
  of the domain if schema memory declares it.
- Schema can evolve into a marketable trust feature: typed, secure, explainable
  graph memory over flexible graph storage.
- Future hosted/service work can expose a typed schema-memory contract without
  requiring users to learn a new database language.

Tradeoffs:

- GRM must manage lifecycle and consistency between graph data and schema
  memory.
- Fresh agents need guidance to distinguish "empty schema, define one" from
  "empty schema, avoid touching an existing graph until the operator confirms."
- Schema memory may initially live outside the graph data store, as it does in
  the current local-file Neo4j MCP slice.

## Open Questions

- Should schema memory eventually be stored inside Neo4j/graph backends as GRM
  metadata nodes and edges, or remain in a GRM-owned metadata store?
- How should schema memory versioning, migrations, compatibility checks, and
  rollback work?
- How should user-defined indexes, saved query objects, explain/profile
  expectations, and agent recall paths be represented in schema memory?
- What authorization model governs who can change schema memory in a hosted
  service?

## Guidance

When adding future backend or service features, prefer designs that preserve the
distinction between:

- data stored in the user's graph, and
- schema memory that communicates and validates the intended shape of that
  graph.

Do not rely on data inference alone for runtime schema. Inference can be useful
as a recovery or suggestion tool, but it cannot distinguish "not part of the
domain" from "part of the domain but no instances exist yet."
