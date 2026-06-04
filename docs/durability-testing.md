# Durability Testing Note

This note captures the current testing stance for persistence durability work.

It is not a promise of universal filesystem safety. It is a guide for how `grm-rs` should build confidence in the current snapshot-plus-append-log persistence model while durability features evolve.

## Current Intent

The immediate goal is to make local session/workspace persistence safer without
changing the simple user-facing session model.

The conservative product claim is:

> Durable local graph memory for agents and projects.

More precisely: after a successful autocommit write returns, the write is
present in either the append log or a checkpoint on a single local filesystem,
assuming one writer owns the session or workspace. Rust workspace autocommit is
provided through the public `Workspace::execute_runtime` execution path; direct
`workspace.state_mut()` calls remain low-level and are not part of the
autocommit claim.

This is a local operational-memory durability claim, not a claim that GRM is a
general-purpose file database. The durable artifact should evolve into a
workspace envelope that lets GRM reopen the same typed memory context with graph
data, runtime schema, schema memory metadata, durable deltas, and recovery
boundaries.

For the currently supported service-backed local target class, including binary
default persistence, JSON opt-in behavior, create/open/reopen expectations, and
unsupported durability cases, see [Local Durability Target Class](local-durability-target.md).

That means focusing on:

- interrupted-write safety
- backup and recovery behavior around snapshot files
- replay from checkpoint plus later append-log records
- safe handling of a truncated final append-log record
- clearer confidence in autocommit on real machines

## Initial Platform Targets

The first obvious real-machine targets are:

- Linux
- macOS

These are the platforms most worth treating as the initial durability bar for local developer workflows.

Windows may matter later, but it does not need to block the first durability pass.

## What We Need Confidence In

Durability confidence should come from testing the failure boundaries that actually matter:

- write temporary file
- flush file contents
- replace the target snapshot
- restart after interruption
- recover from a damaged primary snapshot when a backup exists
- handle repeated autocommit writes without drifting into corruption
- rebuild backend-maintained system indexes from recovered schema and graph state

Durable source-of-truth data currently includes runtime schema definitions, node and edge CRUD operations, typed batch graph operations, checkpoints/snapshots, and recovery metadata. User-defined index definitions are expected to join this set later. Backend-maintained system index contents are derived data and should be rebuilt after recovery rather than trusted as durable state.

This matters more than broad but shallow “it ran on many machines” coverage.

## Filesystem / Environment Scope

The first scoped claim should stay conservative:

- single-machine local workspace envelopes/session files
- normal local filesystem semantics
- one writer at a time

Shared filesystems, network storage, and clustered/multi-writer behavior should be treated as separate durability classes and tested later.

This is not a distributed durability claim, a cloud-service claim, or a multi-writer coordination claim.

## Scale Testing

Durability work should not only be tested on tiny toy sessions.

We also want scale-oriented test data that is still understandable and hand-curated enough to inspect.

Current useful starting point:

- `isms-experiment.grm`, when available locally

That file is a good seed for broader scale-data testing because it already reflects a more realistic domain shape than the smallest demo scripts.

Over time, the test corpus should grow to include:

- larger numbers of models and links
- deeper relationship chains
- denser graphs
- larger property payloads
- repeated autocommit mutation sequences against the same snapshot target

## Recommended Confidence Ladder

1. Unit and integration tests for save, autocommit, backup, shared durability APIs, and recovery logic.
2. Repeatable local failure-injection tests:
   kill during autocommit, truncate files, damage primary snapshots, verify fallback behavior.
3. Manual runs on real Linux and macOS machines.
4. Larger scripted datasets to expose timing, file-size, and recovery edge cases.

## Working Principle

The right goal is not “certainty on every hardware target”.

The right goal is a clear and honest durability claim that matches the environments we have actually tested.
