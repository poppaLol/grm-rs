# GRM-RS Performance Benchmarks

This directory is for repeatable performance benchmarks. Keep correctness tests in
`tests/`; use `benches/` for workloads that should be run explicitly with
`cargo bench`.

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

Compare:

- GRM runtime query commands/API
- SQLite indexed selects and joins

### Persistence Cost

- GRM `save_to_json`
- GRM `save_to_binary`
- GRM `load_from_json`
- GRM `load_from_binary`
- GRM autocommit log append
- GRM compact/checkpoint

Track:

- elapsed time
- output file size
- log file size when applicable

## Dataset Sizes

The checked-in harness currently uses quick-run local datasets:

- 250 users/posts/authored edges for insert throughput
- 1,000 and 10,000 users/posts/authored edges for lookup and traversal
- 1,000 users/posts/authored edges for persistence

Use deterministic generated data so benchmark runs are comparable.
The lookup and traversal benchmarks bulk-load fixtures through one lower-level
transaction so setup time does not dominate indexed read measurements.
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

## Planned Files

- `grm_vs_sqlite.rs`: first comparison of inserts, property lookup, and one-hop traversal.
- `persistence.rs`: save/load/autocommit/compact costs and file sizes.

## Running Benchmarks

```bash
scripts/benchmarks.sh all
scripts/benchmarks.sh grm-vs-sqlite
scripts/benchmarks.sh persistence
scripts/benchmarks.sh quick
scripts/benchmarks.sh scaled
scripts/benchmarks.sh stress
scripts/benchmarks.sh check
```

The SQLite comparator uses `rusqlite` with bundled SQLite, so local benchmark runs
do not require a system SQLite installation.
