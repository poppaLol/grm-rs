# ADR 0010: Use A GRM-Shaped Local Graph Store For The Flight-Deck

Status: Accepted

Date: 2026-08-01

## Context

The first GRM flight-deck UI has landed as a React/Vite/TypeScript browser
application with a small local HTTP gateway. It can use fixture data or load a
bounded service-backed snapshot, render a 2D graph, apply client-side graph
delimiters, inspect selected nodes and edges, and keep a small event-stream
hook alive for future execution animation.

The next flight-deck work needs stronger connection ergonomics. A user should
be able to return to the page and reuse non-secret connection details loaded
from local browser storage instead of retyping the service URL, workspace, mode,
fixture setting, or snapshot limit every time.

At the same time, the flight-deck is likely to grow beyond one component-local
snapshot:

- connection profiles and status;
- workspace/session state;
- schema model catalogues;
- normalized node and edge data;
- graph selection, hover, layout, and delimiters;
- service-backed query/explain/profile results;
- audit and security-context views; and
- execution events such as reads, writes, node creation, and edge traversal.

A generic UI state store alone would hide GRM concepts inside component state.
A full browser GRM runtime would be premature for the next slice. The chosen
direction needs a local store that is shaped like GRM graph state while keeping
the service boundary canonical.

## Decision

The flight-deck will use a GRM-shaped local SPA graph store for browser state,
connection-profile reuse, graph snapshots, and future event application.

The first implementation should be a TypeScript browser store, not a new
canonical GRM runtime. It may use a small proven state library such as Zustand,
or a local React reducer/context if that better fits the current codebase, but
the important boundary is the domain shape:

```text
connection profiles
workspace/session state
schema model catalogue
nodes by stable id
edges by stable id
derived visible graph
selection and layout state
execution/event stream buffer
```

The store is a local cache and projection over typed service responses. It does
not replace the GRM service, runtime session, workspace durability, or typed
operation contract.

The store should expose GRM-shaped actions rather than widget-specific setters,
for example:

```text
loadSnapshot(snapshot)
saveConnectionProfile(profile)
selectConnectionProfile(profileId)
markConnectionStatus(status)
applyGraphFilter(filter)
selectNode(id)
selectEdge(id)
applyExecutionEvent(event)
clearWorkspace()
```

Only non-secret, local-development-safe profile data should be persisted in
browser storage:

- profile name;
- gateway or service base URL;
- workspace ID;
- connection mode;
- fixture mode;
- bounded snapshot limit;
- last selected profile ID; and
- explicit auto-load preference if later added.

The store must not persist raw credentials, private keys, raw certificates,
passwords, bearer tokens, policy table contents, private local filesystem paths,
or unbounded request/response bodies.

## Future Local GRM Runtime Path

The local SPA store should leave room for a later implementation backed by
in-memory `grm-rs`, either through WebAssembly or a local sidecar/session. That
future store could provide real local GRM semantics for typed schema validation,
query/explain/profile, offline snapshots, and event replay.

That future is explicitly deferred. The next slice should design a narrow
TypeScript interface that can later be implemented by a WASM or sidecar-backed
GRM session without pulling Rust/WASM into the immediate connection-profile
work.

## Non-Goals

- No generic "Pinia for graphs" framework.
- No browser-side canonical GRM runtime in the first store slice.
- No Rust/WASM `grm-rs` integration in the first store slice.
- No new public query language.
- No credential, private-key, raw-certificate, token, password, policy-table,
  or unbounded body persistence.
- No production authentication, authorization, hosted workspace, or admin
  control-plane claim.
- No 3D/spatial navigation requirement.

## Consequences

Positive consequences:

- Returning users can reuse safe local connection details.
- React component state stays smaller as graph, connection, and event behavior
  grows.
- Query/explain/profile, audit, and security-context views get a shared local
  graph/state foundation.
- Execution animation can apply events to one normalized graph projection.
- A future in-memory `grm-rs` local runtime can slot behind a stable store
  interface.

Tradeoffs:

- The flight-deck owns a small domain store instead of relying only on component
  state.
- Store shape and persistence migrations need tests.
- The store must be careful not to imply local browser state is authoritative.
- A later WASM/sidecar-backed store may require interface changes if the first
  TypeScript shape is too UI-specific.

## Required Proof For The First Implementation

The first implementation should be proven through frontend tests and build
checks that cover:

- profile persistence and restoration from browser storage;
- migration from the existing single connection-settings storage key if needed;
- no persistence of secret-like fields;
- snapshot normalization into node and edge maps keyed by stable string IDs;
- derived visible graph behavior for the existing graph delimiter/filter;
- selected node/edge behavior across filter and snapshot changes; and
- successful `npm run build`.

If service interaction changes, keep the proof at the public gateway/service
surface rather than private runtime internals.

## Open Questions

- Should the first store use Zustand, React reducer/context, or another small
  library?
- Should profile data stay in `localStorage`, move to IndexedDB, or use both
  with a migration boundary?
- When should auto-loading a restored profile be allowed, and what UI signal is
  required before network activity?
- What minimum interface should a future WASM or sidecar-backed in-memory
  `grm-rs` store implement?

