# Query And Persistence Optimization Path

This note captures the next-phase performance direction after the first
`grm_vs_sqlite` Criterion comparisons.

The current benchmark story is encouraging: GRM is already very fast on
graph-native read primitives such as indexed property lookup, adjacency lookup,
and one-hop traversal. SQLite remains a very strong baseline for flat bulk
inserts, which is expected. The point of the next phase is not to chase SQLite
on every relational workload, but to make sure GRM consistently chooses and
preserves the fast graph paths it was built for.

## Benchmark Interpretation

Use SQLite as a yardstick, not as the product shape.

SQLite bulk insert is difficult to match because it is highly optimized for
flat row insertion into B-tree-backed tables inside a transaction. GRM insert
work naturally does more graph-specific bookkeeping:

- typed ID allocation
- node and relationship storage updates
- label and relationship-type indexes
- adjacency indexes
- property-index cache invalidation or maintenance
- schema and endpoint validation

That work is not waste when it pays for fast graph-shaped reads later.

The healthy interpretation of the current numbers is:

- SQLite is still the brutal baseline for flat transactional bulk writes.
- GRM's local in-memory paths are already strong for graph-shaped reads.
- Insert performance should keep improving, but not by weakening read
  correctness or graph topology maintenance.
- Transaction-overlay reads deserve focused attention because they can hide
  whole-store materialization or scan costs behind otherwise fast primitives.

## Query Optimization Direction

The immediate goal is a small planner before a large optimizer.

Queries from CLI, `GraphQuery`, and any Cypher-like surface should normalize
into an internal plan that can choose anchors, order operations, and avoid
unnecessary materialization.

First useful planning rules:

- prefer ID lookups over property lookups
- prefer indexed property filters over label scans
- prefer typed adjacency expansion over relationship scans
- choose the smallest known candidate set as the starting anchor
- push labels, relationship types, predicates, `limit=<int>`, and projections
  as early as possible
- avoid loading full nodes or relationships when only IDs, counts, or selected
  fields are needed
- order multi-hop traversals from the most selective anchor outward

A first cost model can be deliberately simple:

1. ID lookup
2. indexed property lookup
3. typed adjacency expansion
4. label scan
5. relationship-type scan
6. full graph scan

This should be enough to prevent many accidental slow plans while keeping the
implementation understandable.

## Explain And Profile

GRM should grow `session.explain` and `session.profile` style introspection for
query execution.

Current groundwork is intentionally internal: backend contracts now document the
`QueryResult`/`QueryRow` shape, transaction visibility expectations, practical
error categories, current backend-assigned `i64` IDs, and lightweight capability
hints. A small execution-plan vocabulary (`NodeById`, `NodeLabelScan`,
`NodePropertySeek`, `NodeCheck`, `NodeFilter`, `ExpandOut`, `ExpandIn`,
`ExpandBoth`, `Return`) gives later planner and profile work stable words to use
in tests and logs. This does not add CLI `session.explain` or
`session.profile` commands yet.

`session.explain` should show the selected logical or physical plan without
running the query. `session.profile` should run the query and report
per-operator row counts and timings.

Example shape for the current CLI query language:

```text
session.profile node.find User name="user-000500" via=out:Authored:Post

Plan:
1. NodeIndexSeek User.name = "user-000500"   rows=1   time=1.2us
2. ExpandOut Authored                        rows=1   time=0.4us
3. NodeById Post                             rows=1   time=0.7us
4. Return Node                               rows=1   time=0.2us

Total: 2.8us
```

This gives users a way to understand query behavior and gives maintainers a way
to see whether planner changes actually select better execution paths.

## Scheduler And Execution Model

The first scheduler should be conservative and single-process friendly.

Useful responsibilities:

- execute plan operators in an order chosen by the planner
- stream rows between operators where possible
- stop early for `limit=<int>`
- keep projection narrow until final result formatting
- let aggregation operators consume compact inputs instead of full materialized
  rows
- report operator-level metrics for `session.profile`

Parallel execution can wait. The larger near-term win is to avoid unnecessary
work and preserve index-backed execution.

## Fast Aggregations

Common CLI and Cypher-like aggregation cases should avoid materializing all
matching entities when a cheaper count or reduction path exists. The examples
below use Cypher-like notation to show query intent; matching CLI syntax should
be designed as this feature becomes available.

Important first targets:

- `COUNT(*)`
- `COUNT(n)`
- count by label
- count relationships by type
- degree counts from adjacency indexes
- `MIN`, `MAX`, `SUM`, and `AVG` over numeric properties where index or compact
  property iteration makes sense
- grouped counts such as `MATCH (u:User) RETURN u.age, count(*)`

Potential fast paths:

- label count from label index length
- relationship-type count from relationship-type index length
- outgoing or incoming degree from adjacency index length
- indexed exact-match count without building full result nodes
- projection-only aggregation over compact property values

The guiding rule is that aggregation plans should consume the smallest possible
representation: counts, IDs, or scalar values before full rows.

## Persistence Optimization Direction

Persistence work should follow the transaction-delta shape rather than logging
whole snapshots for every durable operation.

Near-term goals:

- finish indexed transaction overlay/read-view support for graph execution,
  traversal, deletes, and property-indexed reads
- keep operation deltas explicit enough that a future WAL can record compact
  changes instead of whole session images
- keep autocommit simple for users while making its implementation more
  append-friendly and recoverable
- preserve clear recovery behavior for damaged snapshots and replay logs
- benchmark save, load, compact, autocommit append, recovery, and file sizes as
  separate concerns

Persistence should stay honest about its durability class:

- local filesystem
- one writer at a time
- clear interrupted-write and recovery behavior
- broader multi-process or network-filesystem claims only after targeted tests

## Benchmark Additions

Extend the benchmark suite to cover optimizer and persistence decisions directly.

Query benchmarks:

- naive versus planned execution for the same `GraphQuery`
- indexed anchor selection versus scan-first execution
- multi-hop traversal with selective start, middle, and end filters
- count by label
- count by relationship type
- degree count
- grouped count over a node property
- projection-only query versus full-node materialization

Persistence benchmarks:

- autocommit append cost
- WAL or replay-log recovery cost
- checkpoint or compact cost after many small writes
- load time after snapshot plus replay log
- file size growth across repeated mutation sequences

Regression checks:

- keep `tx_overlay_reads_*` in the main comparator
- add a targeted overlay property-lookup benchmark for the 10k case that
  recently showed regression risk
- keep benchmark names stable so Criterion baselines remain useful

## Suggested Phase Order

1. Define a compact internal plan representation for CLI queries, `GraphQuery`,
   and future Cypher-like input.
2. Add `session.explain` for plan inspection.
3. Add `session.profile` with per-operator row counts and timings.
4. Implement simple cost-based anchor selection and hop ordering.
5. Add non-materializing aggregation operators for counts and degree queries.
6. Finish indexed transaction overlay/read-view paths that avoid whole-store
   materialization.
7. Evaluate WAL/autocommit changes once operation deltas are stable.
8. Add benchmarks that compare naive, planned, and persistence-aware execution.

## Working Principle

The next phase should turn fast backend primitives into a fast query system.

Optimize writes where it is reasonable, but do not erase the graph-specific
bookkeeping that makes traversal cheap. The product win is not being a faster
SQLite clone. It is being a local graph engine that makes graph-shaped work
predictably fast, inspectable, and durable.
