# Local Durability Target Class

Status: Current implementation note

Date: 2026-06-02

This document states the current local durability target for GRM
service-backed workspace storage. It is intentionally narrow. It describes what
the local gRPC workspace shell, CLI service-backed mode, MCP gRPC mode, Python
service mode, and Rust service client can rely on today when they use a
GRM-owned local workspace root.

It does not define a hosted durability class, a final workspace envelope
format, or a production service contract.

## Current Target

The supported target class is:

> single-writer local filesystem durability for GRM-owned service-backed
> workspaces, using local autocommit checkpoints plus an append log.

More precisely, after a supported workspace mutation returns successfully
through `ExecuteWorkspace`, the mutation's durable operation is present in
either the workspace checkpoint file or the workspace append log, assuming one
service process owns writes for that workspace file on a normal local
filesystem.

This is the same local durability family used by runtime workspace autocommit:
`Workspace::execute_runtime` executes a typed runtime request and appends the
durable operations returned by `SessionState::execute_runtime`.

## Workspace Files

Service-backed local workspaces are addressed by opaque workspace refs, not by
client-supplied server filesystem paths. The local service maps a workspace ref
to a file under the server-configured workspace root:

- binary workspace: `<workspace-ref>.bin`
- JSON workspace: `<workspace-ref>.json`
- append log: `<workspace-file>.log`
- backup checkpoint: `<workspace-file>.bak`

Binary persistence is the default for checked service-backed clients and adapter
modes. JSON workspace persistence remains an explicit opt-in for debugging,
inspection, tests, and interoperability-friendly local workflows. JSON
interchange export/import is separate from workspace persistence; interchange
JSON moves graph data between tools and is not the primary durability target for
service-backed workspaces.

## Create, Open, Reopen

`CreateWorkspace` in local autocommit mode creates a fresh in-memory workspace,
maps the workspace ref to the local workspace file, enables autocommit, and
immediately writes a checkpoint. Creating with an existing workspace ref
reinitializes/replaces that service-managed file by writing a fresh checkpoint
and clearing the previous append log; it is not a safe attach/open path, merge
operation, or multi-writer attach operation.

`OpenWorkspace` with a workspace ref loads the checkpoint file, replays any
complete append-log records after that checkpoint, and then re-enables
autocommit for future `ExecuteWorkspace` writes.

Reopen behavior is covered by current service-backed smoke tests for the checked
operation subset: schema definition/listing, schema-aware node and edge CRUD,
simple find, traversal-backed `node.find` result shapes, `return=edge`,
explain/profile for typed find shapes through service-backed adapters, batch,
and close/reopen verification.

## Autocommit And Checkpoint Behavior

Enabling autocommit writes a checkpoint of the current workspace state. Later
successful mutations append durable operation records in order. Reads and
requests that return no durable operations do not write to the append log.

The current workspace autocommit implementation checkpoints again after 8
pending durable operation records. A checkpoint rewrites the workspace snapshot
atomically with a backup file and clears the append log. Append-log writes are
newline-delimited durable operation records; the implementation flushes and
syncs each appended record. On recovery, a final log record without a newline is
treated as a possible torn write and ignored, while earlier complete records
remain replay input. A malformed complete log record aborts load.

The current implementation uses a single in-process mutex around the local gRPC
workspace service. That serializes requests inside one service process, but it
is not a cross-process lock, lease, transaction manager, or distributed
coordination mechanism.

## Guaranteed Today

Within the target class above, GRM currently supports:

- local create/open/reopen of service-managed workspaces by workspace ref
- binary workspace files by default for checked service-backed clients
- explicit JSON workspace files when selected by the caller
- checkpoint plus append-log recovery for complete durable operation records
- backup-file fallback when the primary checkpoint cannot be deserialized
- ignoring a truncated final append-log record during recovery
- persistence of current durable source-of-truth state covered by durable
  operations: runtime schema definitions, node CRUD, edge CRUD, and typed batch
  graph operations
- rebuildable backend-maintained system index contents rather than treating
  those contents as durable source of truth

## Best-Effort Or Implementation-Dependent

These behaviors are useful and currently implemented, but should not be
overstated as broader product guarantees:

- Atomic checkpoint replacement depends on normal local filesystem rename and
  directory sync semantics.
- Backup recovery helps when the primary checkpoint is unreadable, but it is not
  a general corruption-repair system.
- Append-log sync gives local crash-recovery evidence for completed records, but
  it is not a full WAL with transaction IDs, epochs, checksums, or replay
  fencing.
- Docker Compose persistence depends on the `grm-workspaces` volume. Removing
  the volume removes local workspace files.
- The in-process service mutex serializes one running service instance; it does
  not protect against another process writing the same workspace file.

## Not Supported Yet

The current local durability target does not include:

- hosted or cloud durability
- RBAC or per-operation authorization, quotas, audit, production certificate
  lifecycle, or service lifecycle management
- multi-writer coordination
- cross-process file locking or leases
- distributed consensus, replication, failover, or clustering
- network/shared-filesystem durability claims
- a final GRM workspace envelope format
- a full WAL/replay design with durable epochs, operation IDs, checksums,
  compaction metadata, or recovery reports
- guaranteed recovery from arbitrary file corruption, disk loss, or operator
  deletion
- universal backend portability or conformance guarantees
- Neo4j-backed service workspaces
- direct non-workspace RPC-family parity outside `ExecuteWorkspace`

## Future WAL/Replay Path

Future durability work should strengthen this local target by replacing the
current compact append-log mechanics with an explicit WAL/replay model:

- durable operation IDs or sequence numbers
- checkpoint and compaction epochs
- replay boundaries and recovery reports
- checksums or record integrity markers
- clearer handling for partial batches and malformed records
- failure-injection tests for interrupted writes and restarts
- eventual capability reporting for durability, snapshots, recovery, and
  multi-writer behavior

That work is the next improvement path. It is not current behavior.

## Related Docs

- [Durability Testing Note](durability-testing.md)
- [GRM gRPC Docker Quick Start](grpc-quickstart.md)
- [GRM gRPC Docker Service](grpc-docker-service.md)
- [Service Boundary Design Spike](service-boundary-design.md)
- [ADR 0005: Use Graph Workspaces And Durable Envelopes](adr/0005-graph-workspace-and-durable-envelope.md)
- [Import / Export](import-export.md)
