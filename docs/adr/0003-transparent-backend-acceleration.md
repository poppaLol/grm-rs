# ADR 0003: Transparent Backend Acceleration From Profiled Workloads

Status: Accepted

Date: 2026-05-22

## Context

GRM is moving toward a typed service backend where users and agents send
structured graph operations rather than choosing physical execution mechanics.
As graph sizes grow, simple system indexes and adjacency lookups will not be
enough for every workload. Some workloads will benefit from derived
acceleration structures such as relationship-type adjacency matrices, cached
projections, specialized property indexes, or future GraphBLAS-style sparse
matrix execution.

Systems such as FalkorDB show the value of sparse matrix representations for
fast graph traversal and graph algorithms. GRM should learn from that direction
without exposing matrix construction or GraphBLAS mechanics as the primary user
contract.

## Decision

Future GRM backends should support transparent backend-managed acceleration
driven by planner/profile data and scheduler policy.

Users should continue to send typed graph operations. The backend service may
observe workload patterns, profile expensive queries, and maintain derived
acceleration structures that improve execution. These acceleration structures
should be visible through explain/profile and backend status, but normal users
should not need to manually choose GraphBLAS, sparse matrix projections, or
other physical execution strategies.

The intended execution shape is:

```text
typed request
  -> planner
  -> profiler and metrics
  -> scheduler policy
  -> derived acceleration manager
  -> indexes, adjacency matrices, cached projections, or algorithm state
  -> backend execution
```

## Source Of Truth

GRM should preserve a strict distinction between durable truth and derived
acceleration:

- graph data is source of truth
- schema memory and acceleration policy may be durable metadata
- index definitions may become durable metadata
- acceleration contents are derived and rebuildable

WAL and recovery should record schema, graph operations, and explicit policy or
index definitions. They should not depend on raw derived matrix/index pages as
the only source of truth. Persisted acceleration artifacts may be introduced
later as checkpoint optimization, but they should remain disposable and
rebuildable from graph data plus metadata.

## User-Visible Behavior

Users should experience acceleration as:

- faster traversals and graph recall
- profile output that identifies expensive steps
- explain/profile metadata showing selected access paths or accelerators
- backend status that reports accelerator health, freshness, memory budget, and
  rebuild state
- optional admin controls for policy, budget, rebuild, pinning, or disabling
  acceleration

The first user-facing language should be policy-oriented, not mechanism-first.
For example:

- enable or disable automatic acceleration
- set memory or rebuild budgets
- inspect accelerator catalog
- rebuild stale derived structures
- pin an important projection later

Avoid exposing a first-class user requirement such as "create a GraphBLAS
matrix" for ordinary workloads.

## Relationship To Algorithms

GRM should eventually grow a graph algorithms layer, but it should start from
GRM's typed graph model rather than from a full custom GraphBLAS implementation.

A future `grm-algorithms` or equivalent crate could provide algorithms such as:

- degree statistics
- bounded traversal helpers
- connected components
- PageRank or ranking algorithms
- neighborhood expansion helpers for GraphRAG

Those algorithms can initially run over existing backend/read APIs. Later they
can use a matrix/accelerator abstraction backed by simple Rust sparse
structures, SuiteSparse GraphBLAS through FFI, or a custom GRM matrix engine if
benchmarks justify it.

GRM should not commit to building a full Rust GraphBLAS implementation unless
that layer becomes central to the product and cannot be satisfied through an
abstraction over existing implementations.

## Consequences

Positive consequences:

- Users get performance improvements without learning physical execution
  details.
- Explain/profile remains central to trust: acceleration is visible, not magic.
- Hosted GRM can become more valuable over time by learning real workload
  patterns.
- Derived structures can be rebuilt after recovery instead of complicating WAL
  guarantees.
- GRM keeps room for GraphBLAS-style execution without coupling the service API
  to GraphBLAS concepts.

Tradeoffs:

- Scheduler and accelerator policy become real backend responsibilities.
- Profile data, cost models, and freshness tracking need careful design.
- Background work must respect memory, CPU, and tenant/workspace isolation
  budgets.
- Explain/profile must avoid overstating acceleration when a backend cannot
  support a given structure.
- Automatic acceleration may create operational surprises unless status and
  controls are clear.

## Guidance

Future acceleration work should follow these rules:

- Keep typed graph operations as the user and service contract.
- Treat acceleration structures as derived, rebuildable state.
- Make accelerator use visible in explain/profile.
- Let scheduler policy decide when to build or refresh derived structures.
- Add admin controls gradually around policy and health rather than exposing
  low-level matrix mechanics first.
- Benchmark before choosing SuiteSparse GraphBLAS FFI, Rust sparse libraries, or
  a custom engine.
- Do not let acceleration policy weaken durability or recovery claims.

## Open Questions

- What profile signals should trigger automatic acceleration?
- What budget model should govern memory, CPU, rebuild frequency, and staleness?
- Which accelerator should come first: relationship adjacency matrices,
  property projections, cached traversal neighborhoods, or algorithm-specific
  state?
- Should acceleration policy live in schema memory, backend metadata, service
  admin configuration, or a mix of those?
- How should multi-tenant hosted GRM isolate accelerator cost and memory?
- What explain/profile vocabulary should describe matrix-backed or
  GraphBLAS-backed execution without requiring users to understand GraphBLAS?

