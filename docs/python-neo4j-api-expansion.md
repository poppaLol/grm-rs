# Python API Expansion Toward Neo4j

Status: portable graph-session core implemented

## Goal

Grow the Python package from a local runtime-session binding into an API surface
that can support a live Neo4j backend without making early Python users pay for
that complexity.

The current Python prerelease can keep iterating asynchronously with a small
in-the-know group. New API work should be shaped by the backend requirements
Neo4j introduces: network IO, async operations, explicit commits, backend
capabilities, and backend-neutral IDs.

## Current Shape

`grm_rs.Session` currently represents:

- an in-memory `SessionState`
- blocking Python methods
- optional blocking local file autocommit
- local JSON and binary save/load helpers

That shape is useful for scripts, smoke tests, demos, and early feedback. It
should remain the simple default.

## Direction

Introduce backend-aware Python APIs without breaking the local default.

Expected surface:

```python
session = grm_rs.Session()
```

Local, blocking, in-memory session. This remains the friendly entry point.

```python
session = await grm_rs.AsyncSession.neo4j(
    uri="localhost:7687",
    user="neo4j",
    password="...",
)
```

Async-first live backend session for Neo4j and other networked backends.

The current implementation provides:

```python
session = await grm_rs.AsyncNeo4jSession.connect(
    uri="localhost:7687",
    user="neo4j",
    password="...",
)
```

`AsyncNeo4jSession` remains an async convenience wrapper over the blocking
extension class, with the same schema inspection, CRUD/find, atomic batch, and
native query methods.

```python
session = grm_rs.Session.neo4j(
    uri="localhost:7687",
    user="neo4j",
    password="...",
)
```

Optional blocking convenience wrapper for notebooks, one-off scripts, and
callers that are not already inside an event loop.

## API Semantics

Keep local snapshot persistence distinct from backend durability.

- `save_json()` and `save_binary()` mean local session snapshot persistence.
- `export_json()` means portable interchange output.
- `commit()` means commit a backend transaction.
- `autocommit=True` on local sessions means blocking local-file convenience.
- Neo4j writes should not serialize the whole session after every mutation.

For Neo4j, write durability should come from backend transactions, not local
snapshot autocommit.

## Implemented Shared Core

Schema-aware Neo4j CRUD, simple find, edge lookup, validation, and atomic batch
orchestration now live in the shared Rust runtime Neo4j module. MCP and Python
call that implementation. Adapter-only filter parsing stays at the adapter
edge, while structured runtime requests remain the operation contract.

`GraphSession` is the portable synchronous Python protocol. It includes:

- node and link model create/list
- node and edge create/find/update/delete
- atomic-only batch writes
- opaque `GraphId` values and capability discovery

`WorkspaceGraphSession` adds traversal and explain/profile capabilities.
Its `WorkspaceBatchCapability` also adds non-atomic batching; the portable
`GraphSession.batch()` signature deliberately does not accept `atomic=False`.
`Neo4jGraphSession` adds native `execute_query`; native Cypher is intentionally
outside the portable contract.

Neo4j schema memory remains GRM-owned session/catalog state and is not written
into the user's graph. Atomic batches stage schema changes and install them only
after the single Neo4j transaction commits.

## Remaining Rust Work

Before Python can write directly to Neo4j:

1. Add a minimal `Neo4jBackend` implementing `GraphBackend` and `GraphTx`. (started)
2. Expose a first Python Neo4j session surface. (complete)
3. Make `SessionState` backend-pluggable instead of hard-wired to
   `GraphClient<InMemoryBackend>`.
4. Evolve the lightweight Python capability names into richer backend/runtime
   capability descriptions when a concrete consumer requires them.
5. Clean up backend identity enough for Python to handle non-`i64` IDs without
   leaking backend internals.
6. Broaden backend conformance testing beyond the focused portable-session
   coverage when semantics genuinely align.

## First Python Milestone

The first useful milestone is not full Neo4j parity. It is:

- connect to Neo4j from Python
- define simple node and relationship models
- create nodes
- create relationships
- find nodes by ID or simple property filters
- run one-hop traversal backed by translated `GraphQuery`
- commit or rollback explicitly

This is enough to validate the async Python shape and the live backend contract
without committing to every future query feature at once.

## Non-Goals

- Do not replace the local `Session()` default.
- Do not make public PyPI publishing a prerequisite for API iteration.
- Do not expose Rust generic or macro-heavy APIs directly to Python users.
- Do not treat GitHub Release wheels as a stable public package contract.
- Do not hide network IO behind local-file `autocommit` semantics.

## External Verification

With a dedicated Neo4j test endpoint:

```bash
NEO4J_URI=... NEO4J_USER=... NEO4J_PASSWORD=... \
  cargo test --test neo4j_bolt_smoke shared_neo4j -- --ignored --nocapture
```

After `maturin develop`, the Python live-backend smoke remains:

```bash
NEO4J_URI=... NEO4J_USER=... NEO4J_PASSWORD=... \
  python tests/python_neo4j_smoke.py
```

The local gRPC binding test is:

```bash
cargo test -p grm-rs-python --lib
```
