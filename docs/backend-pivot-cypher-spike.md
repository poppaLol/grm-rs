# Backend Pivot: Cypher Spike Before Deeper In-Memory Storage Work

Status: accepted and initially validated
Branch: `cypher_spike`
Decision point: before extending in-memory transaction internals further

## Context

`grm-rs` has been moving toward a stronger in-memory transaction model. The
previous next-step framing was "extend delta-style transactions" so graph
execution, traversal, deletes, and property reads no longer force a whole-store
working copy.

That is still important, but the framing was too close to a storage-engine
conversation. The in-memory backend is not currently designed around index-free
adjacency. It is an indexed local graph store with:

- label indexes
- property indexes
- relationship-type indexes
- outgoing and incoming adjacency indexes
- a backend-agnostic `GraphQuery` IR above it

Moving toward true index-free adjacency would be a much larger storage-engine
departure. It would affect persistence, deletion, compaction, recovery,
concurrency, identity stability, and memory layout. That is not the right next
bet without stronger evidence.

## Decision

The next branch of work should start with a **Cypher translator spike**, not a
physical adjacency redesign.

The immediate goal is to check whether the current `GraphQuery` contract is
portable enough to map cleanly onto Neo4j-style execution.

The first spike started small and non-networked:

1. Add an isolated `GraphQuery` to Cypher translation path.
2. Cover root node matches, one-hop traversal, direction, relationship type,
   property filters, return node/relationship, limit, and offset.
3. Use tests or snapshots that compare `GraphQuery` inputs to expected Cypher
   strings and parameters.
4. Keep normal tests independent of a live Neo4j server.

That spike has now been extended with an ignored live Bolt smoke test. The smoke
test has successfully connected to a local Neo4j instance at
`host.docker.internal:7687`, seeded a small `User -[:AUTHORED]-> Post` graph,
executed Cypher generated from `GraphQuery`, verified the returned node, and
cleaned up the inserted data.

The live check is intentionally opt-in. It is not part of normal `cargo test`.

After that check, continue the in-memory work as an **indexed transaction
overlay**, not as index-free adjacency.

## Current Validation State

Implemented:

- `graph_query_to_cypher(&GraphQuery) -> Result<CypherQuery>`
- `CypherQuery { text, params }`
- named parameters as `BTreeMap<String, serde_json::Value>`
- offline translation tests for:
  - root node matches
  - one-hop traversal
  - incoming any-relationship traversal
  - return node and return relationship
  - limit and offset
  - escaped Cypher names
- ignored Neo4j Bolt smoke test using `neo4rs`

Verified:

- normal Rust test suite passes without Neo4j
- ignored live smoke test has passed against local Neo4j through
  `host.docker.internal:7687`

## Updated Technical Direction

The in-memory backend should stay index-backed for now.

The next in-memory milestone should be:

> Build a transaction overlay/read-view for the indexed in-memory backend.

That means composing `base store + tx delta` during reads without cloning the
whole graph.

Expected helper shape:

- `visible_node(id)`
- `visible_rel(id)`
- `visible_outgoing_ids(id, ty)`
- `visible_incoming_ids(id, ty)`
- equivalent helpers for property and label candidate selection

These helpers should preserve the current public behavior while avoiding
`materialize_working_copy()` in graph execution, traversal reads, deletes, and
property scans where possible.

## Non-Goals For This Pivot

- Do not introduce true index-free adjacency yet.
- Do not redesign persistence around physical adjacency chains yet.
- Do not build a full live Neo4j backend before the translation contract is
  checked.
- Do not couple in-memory internals to Neo4j storage internals.

## Why This Matters

This pivot protects the project from optimizing the wrong layer.

If `grm-rs` is primarily a typed graph workbench with portable backend support,
then `GraphQuery` portability matters more than making the in-memory backend
look like Neo4j internally.

If future benchmarks show indexed adjacency is the limiting factor for the local
engine, then a deeper adjacency redesign can be evaluated with evidence.

## Current Status

The Cypher translator spike and minimal live Neo4j backend prototype are done.
The prototype has Rust/Python smoke coverage and is now a backend-hardening
input rather than a standalone future milestone.

Future priority ordering is centralized in [CLI Session Roadmap](cli-roadmap.md).
This note should capture the backend design rationale; the roadmap should remain
the source of truth for what comes next.

For the Python-facing API direction that should guide the live Neo4j work, see
[Python API Expansion Toward Neo4j](python-neo4j-api-expansion.md).
