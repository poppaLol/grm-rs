# GRM-RS Performance Benchmarks

This directory is for repeatable performance benchmarks. Keep correctness tests in
`tests/`; use `benches/` for workloads that should be run explicitly with
`cargo bench`.

For benchmark sequencing, comparator fairness, and when service results are
appropriate for public claims, see
[Performance Benchmark Methodology](../docs/performance-benchmark-methodology.md).

## Goals

- Measure GRM-RS performance as data volume grows.
- Compare GRM-RS against SQLite for equivalent local graph-shaped workloads.
- Separate in-memory graph costs from persistence costs.
- Track regressions over time with stable benchmark names and datasets.

## Initial Benchmark Scope

### Insert Throughput

- create node models and relationship models
- insert users
- insert posts
- insert authored edges

Compare:

- GRM in-memory session operations
- SQLite inserts inside one transaction
- SQLite inserts with indexes enabled

### Query Latency

- exact property lookup: user by name
- range-style property filter: users by age threshold
- one-hop traversal: user authored posts
- transaction overlay/read-view reads: property lookup, one-hop traversal, and
  `GraphQuery` execution inside a read transaction

Compare:

- GRM embedded in-memory runtime query commands/API
- SQLite indexed selects and joins

### Baseline Runtime Operations

- create and update nodes
- create and update edges
- node property lookup
- edge endpoint lookup
- selective one-hop traversal
- `session.explain` and `session.profile` for `node.find` and `edge.find`

Compare:

- GRM embedded in-memory
- SQLite local, for modest embedded comparator lines where the operation intent
  is close enough to be useful, including user-row create and indexed name lookup

The embedded GRM create/update baseline rows use custom timing. Each measured
iteration populates the target in-memory dataset outside the returned Criterion
duration, then records only the mutation duration against that populated state.
These rows are operation-over-populated-state microbenchmarks, not end-to-end
fixture setup benchmarks.

### Local Insecure gRPC Workspace

- create and update nodes through `GrpcWorkspaceClient`
- create and update edges through `GrpcWorkspaceClient`
- node property lookup
- edge endpoint lookup
- selective traversal
- explain/profile over `ExecuteWorkspace` for `node.find` and `edge.find`

The local gRPC benchmark starts an insecure server on `127.0.0.1:0` and uses a
Criterion/tempfile-created workspace root. It is only a local transport overhead
and workspace demo line. It is not a credible deployable service baseline and
must not be used for public service/database comparison claims before TLS exists.

The local gRPC create/update benchmarks use custom timing. Each measured
iteration creates a temp workspace, defines schema, and populates the target
dataset outside the returned Criterion duration, then records only the mutation
RPC duration against that populated workspace. `close_workspace` also runs
outside the recorded duration. These rows are operation-over-populated-workspace
microbenchmarks, not end-to-end workspace setup benchmarks.

### Persistence Cost

- GRM `save_to_json`
- GRM `save_to_binary`
- GRM `load_from_json`
- GRM `load_from_binary`
- GRM binary workspace checkpoint
- GRM binary workspace reopen
- GRM binary workspace reopen with 7 autocommit replay entries after checkpoint
- GRM autocommit log append
- GRM compact/checkpoint

Track:

- elapsed time
- output file size
- log file size when applicable

The current replay benchmark is
`grm_embedded_in_memory_replay_autocommit_binary_7_entries`. It intentionally
keeps the append log below workspace checkpoint rollover, so treat it as a
small-log replay baseline rather than a general replay-cost claim.

## Dataset Sizes

The checked-in harness currently uses quick-run local datasets:

- 250 users/posts/authored edges for insert throughput
- 1,000 users/posts/authored edges for default insert scaling
- 10,000 users/posts/authored edges for opt-in insert profiling/stress
- 1,000 and 10,000 users/posts/authored edges for lookup and traversal
- 1,000 users/posts/authored edges for persistence

Use deterministic generated data so benchmark runs are comparable.
The lookup and traversal benchmarks bulk-load fixtures through one lower-level
transaction so setup time does not dominate indexed read measurements.
The `tx_overlay_reads_*` group measures read transactions over existing committed
data so whole-store materialization regressions are visible in Criterion output.
It also includes dirty-overlay cases for a property lookup after a transaction
local node update and a one-hop traversal after a transaction local relationship
delete plus create.
A 100,000-user Criterion suite is available through `scripts/benchmarks.sh stress`;
it is intentionally opt-in and should be treated as a stress test, not the
default benchmark scope.

## Benchmark Rules

- Do not print inside timed loops.
- Separate setup from measured work.
- Use temporary directories/files for persistence benchmarks.
- Pre-generate input data before timing.
- Label comparisons clearly when GRM scans are compared to SQLite indexes.
- Keep benchmark names stable so Criterion baselines remain useful.

## Insert Indexing Note

The in-memory graph keeps node property indexes as a lazy derived cache. Node
writes update the graph rows and label index immediately, mark the property
index dirty, and rebuild that property index on the first property-indexed read.
This preserves read-your-writes semantics while avoiding high-cardinality
property-index churn during insert-heavy workloads.

This matters for insert benchmarks: the `insert_*` cases measure write cost
without forcing the first later property-index rebuild. The `property_lookup_*`
cases measure steady-state indexed reads after setup has already paid any lazy
rebuild cost during warmup.

## Transaction Overlay Read-View Snapshot

The in-memory transaction overlay optimization should be validated with:

```bash
cargo bench --bench grm_vs_sqlite tx_overlay_reads_10k
```

On the benchmark run used for this PR, the 10,000-row overlay read cases moved:

- `property_lookup_name_eq`: `1.4278 ms` before to `687.69 ns` after
- `one_hop_outgoing_authored`: `355.38 ns` before to `350.93 ns` after
- `graph_query_user_authored_post`: `459.27 us` before to `751.69 ns` after

The dirty-overlay cases added with the optimization measured:

- `property_lookup_after_tx_update`: `1.5906 us`
- `one_hop_after_create_delete_overlay`: `847.03 ns`

Numbers vary by machine and Criterion sample settings; the important signal is
that property lookup and graph-query root selection no longer scale like
whole-store scans inside a transaction.

## Stress Bulk Insert Snapshot

The 10,000-row insert stress comparison should be validated with:

```bash
GRM_BENCH_STRESS=1 cargo bench --bench grm_vs_sqlite insert_10k
```

On the same PR branch, the 10,000-user/post/authored-edge insert comparison was:

- `grm_repo_bulk_transactions`: `60.282 ms`
- `sqlite_in_memory_transaction`: `21.126 ms`

Criterion reported no meaningful GRM insert change from the existing baseline
(`-0.6232%` middle estimate). SQLite remains faster for this bulk insert shape,
at roughly `2.9x` the GRM throughput.

## Planned Files

- `grm_vs_sqlite.rs`: first comparison of inserts, property lookup, and one-hop traversal.
- `persistence.rs`: save/load/autocommit/compact costs and file sizes.

## Running Benchmarks

```bash
scripts/benchmarks.sh all
scripts/benchmarks.sh grm-vs-sqlite
scripts/benchmarks.sh persistence
scripts/benchmarks.sh local-grpc-workspace
scripts/benchmarks.sh quick
scripts/benchmarks.sh scaled
scripts/benchmarks.sh stress
scripts/benchmarks.sh profile-insert
scripts/benchmarks.sh check
```

`profile-insert` builds the Criterion benchmark, then runs the compiled
benchmark binary through `flamegraph` in Criterion profile mode. Running the
bench binary directly keeps Cargo's own build/resolution work out of the SVG.
The wrapper also sets `GRM_BENCH_PROFILE_GRM_INSERT_ONLY=1`, which registers
only GRM bulk insert benchmarks during profiling so SQLite and read benchmark
setup stay out of the sampled process.
By default it profiles `insert_10k/grm_repo_bulk_transactions` for 10 seconds
and writes the SVG beside Criterion output:

```bash
target/criterion/insert_10k/grm_repo_bulk_transactions/flamegraph.svg
```

The wrapper enables bench debuginfo with `CARGO_PROFILE_BENCH_DEBUG=true` so
flamegraphs include useful Rust symbols. On Linux, `cargo flamegraph` also needs
`perf` from the system performance tools package.

Override the benchmark filter, profile duration, or SVG path when needed:

```bash
scripts/benchmarks.sh profile-insert insert_1k/grm_repo_bulk_transactions
PROFILE_TIME=30 scripts/benchmarks.sh profile-insert
FLAMEGRAPH_OUTPUT=target/flamegraph.svg scripts/benchmarks.sh profile-insert
```

The SQLite comparator uses `rusqlite` with bundled SQLite, so local benchmark runs
do not require a system SQLite installation.

## Next Optimization Phase

The next performance phase is tracked in
[docs/query-persistence-optimization.md](../docs/query-persistence-optimization.md).
That note captures how to interpret the SQLite comparison, where query planning
and profiling should go next, and which persistence benchmarks should be added
as the in-memory backend moves toward append-friendly durability.
