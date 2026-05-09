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

## Priority Order From Here

1. Cypher translator spike. (done)
2. Finish the indexed in-memory transaction overlay/read-view so graph execution,
   traversal reads, deletes, and property scans no longer need to materialize a
   whole-store working copy in common paths.
3. Bring Python and MCP closer to parity over schema, CRUD, traversal,
   import/export, and batch operations.
4. Add a minimal live Neo4j backend prototype that can execute translated
   `GraphQuery` values, then grow it toward a fully featured Cypher-compliant
   backend with shared query/repository tests.
5. Clean up the backend contract around result rows, error mapping,
   transaction semantics, backend capability reporting, and IDs.
6. Move identity handling from the current mostly-`i64` shape toward a
   backend-neutral model that can support Neo4j IDs and UUID-style IDs without
   leaking backend details through repository APIs.
7. Revisit durability and WAL design after the transaction delta shape is
   stable, so the local backend can move toward resilient, Redis-like operation
   using compact operation deltas instead of whole snapshots.
8. Build demo scenarios that show ORM-like typed Rust usage, query-like
   integrations, Python workflows, and equivalent MCP workflows.
9. Consider a deeper adjacency redesign only if benchmarks show that the
   indexed store, transaction overlay, and query path are still bottlenecked by
   the current adjacency layout.

For the Python-facing API direction that should guide the live Neo4j work, see
[Python API Expansion Toward Neo4j](python-neo4j-api-expansion.md).
