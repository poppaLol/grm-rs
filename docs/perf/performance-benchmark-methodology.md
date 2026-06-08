# Performance Benchmark Methodology

This document describes the benchmarking sequence for the current engine
acceleration work.

The goal is to establish useful performance truth without implying deployment,
security, durability, or backend parity claims that GRM has not implemented and
tested yet.

## Live Database Safety

Performance work may connect to live databases. Some live databases may contain
GRM project memory: the shared SOML context used by Laurie and agents.

Before any benchmark setup, cleanup, import, restore, reset, or destructive
operation against a live database, identify whether the target could be shared
project memory. If there is any doubt, pause for explicit user confirmation.

This applies to Neo4j, Postgres, Mongo, GRM service workspaces, local workspace
files, mounted Docker volumes, and any configured benchmark database.

Do not rely on backups as permission to wipe data. The benchmark harness should
prefer disposable databases, benchmark-specific labels/prefixes, temporary
workspace roots, and isolated Docker volumes.

## Sequence

Benchmarking should proceed in three stages.

1. Establish the current baseline.
2. Add a narrow TLS-capable service path.
3. Run public client/server comparator benchmarks.

The first stage can start immediately. It should measure the embedded engine and
local insecure gRPC service path so the project has before-numbers for the code
that exists today.

The third stage should not be treated as public evidence until GRM has a
TLS-capable service line. Insecure gRPC measurements remain useful, but only as
local/demo transport overhead measurements. The local TLS line uses generated
or externally supplied certificate material through `GRM_SERVICE_TLS_SERVER_CERT`
and `GRM_SERVICE_TLS_SERVER_KEY` on the server, with
`GRM_SERVICE_TLS_CLIENT_CA_CERT` requiring client authentication. Clients use
`GRM_SERVICE_TLS_CA_CERT`, `GRM_SERVICE_TLS_DOMAIN_NAME`,
`GRM_SERVICE_TLS_CLIENT_CERT`, and `GRM_SERVICE_TLS_CLIENT_KEY`. Tests must
generate short-lived private keys outside the repository.

## Benchmark Lines

Use separate benchmark lines so results do not blur different deployment shapes.

| Line | Purpose | Public comparison suitability |
| --- | --- | --- |
| GRM embedded in-memory | Engine floor and local runtime baseline | Useful for engine claims, not service deployment claims |
| GRM local gRPC insecure | Local transport overhead and demo behavior | Useful for development notes only |
| GRM local gRPC mutual TLS | Credible secured GRM service baseline | Required before public client/server database comparisons |
| SQLite local | Embedded SQL baseline | Fair embedded/local comparison |
| Postgres Docker | SQL client/server baseline | Fair only against GRM TLS service line |
| Mongo Docker | Document client/server baseline | Fair only against GRM TLS service line |
| Neo4j Docker | Graph client/server baseline | Fair only against GRM TLS service line |

## Representative Workloads

The baseline should cover graph-shaped operations that GRM intends to make fast,
plus enough write and persistence behavior to reveal real costs.

Measure at least:

- bulk create nodes and edges
- single node create
- single edge create
- indexed node property lookup
- range-style node property filter
- edge endpoint lookup
- one-hop traversal
- two-hop traversal
- selective traversal with root, edge, and end filters
- high fan-out traversal
- node property update, including indexed-property update
- edge update
- node and edge delete
- explain and profile overhead
- binary save/load or checkpoint/reopen
- append-log replay after checkpoint

Use deterministic generated data. Include at least one small dataset and one
larger dataset so startup/setup effects do not masquerade as engine behavior.

## Like-For-Like Comparator Rules

Compare operation intent, not implementation internals.

For example:

| Intent | GRM | SQL | Mongo | Neo4j |
| --- | --- | --- | --- | --- |
| Node create | create typed node | insert row | insert document | create labelled node |
| Edge create | create typed edge | insert join/edge row | insert edge/reference document | create relationship |
| Property lookup | node find by property | indexed `WHERE` | indexed `find` | indexed label/property lookup |
| One-hop traversal | typed traversal | indexed join | edge lookup plus target lookup | relationship traversal |
| Two-hop traversal | typed multi-hop traversal | chained joins | chained edge/reference lookups | path traversal |
| Degree count | adjacency count | count edge rows | count edge documents | relationship degree/count |

Each comparator should receive the obvious default index needed for the tested
operation. A benchmark that compares GRM indexes with unindexed SQL, Mongo, or
Neo4j paths is not a fair benchmark.

## Measurement Rules

Keep setup outside timed loops unless setup is the workload being measured.

Record enough context to reproduce results:

- GRM commit
- benchmark command
- dataset size and shape
- benchmark line
- comparator versions
- TLS or insecure mode
- persistence format
- database target and whether it is disposable or protected project memory
- machine notes when relevant

Use stable benchmark names so Criterion baselines remain useful.

Do not mix embedded results and service results in one headline. A fast embedded
engine result does not prove a fast secured service deployment.

## Interpreting Results

SQLite is a brutal baseline for flat transactional inserts and should be treated
as a yardstick, not as GRM's product shape.

Neo4j is the most useful graph-native comparator, but it is still not identical
to GRM. GRM is measuring typed graph operation paths and workspace behavior, not
Cypher feature breadth.

Mongo is useful for document/reference access patterns, especially where users
might otherwise model graph-like state as documents plus references.

Postgres is useful for client/server SQL joins, counts, and indexed lookup
behavior.

The acceleration slice should choose one or two targets from measured evidence.
Possible targets include derived property indexes, adjacency/projection caches,
or GraphBLAS-style matrix execution. Any such structures must remain rebuildable
derived state and visible through explain/profile or backend-status style
orientation.

## Current Non-Goals

This methodology does not claim:

- hosted durability
- multi-writer coordination
- authentication or authorization
- production certificate lifecycle
- universal backend parity
- a final GRM/SOML protocol conformance model

Those require separate implementation and test evidence before they can be used
in public claims.
