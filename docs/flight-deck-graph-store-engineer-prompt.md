# GRM Flight-Deck Graph Store Engineer Prompt

## Goal

Implement the first GRM-shaped local graph store for the flight-deck.

The store should let returning users reuse safe local connection details, keep
graph state normalized, and give future query/explain/profile, audit, security
context, and execution-animation work a stable local foundation.

## Why This Matters

The flight-deck is now beyond a spike. Component-local state is enough for the
first snapshot view, but it will become awkward as connection profiles, graph
delimiters, selected graph items, service status, and execution events grow.

Build a local SPA store that mirrors GRM concepts while preserving the canonical
boundary: typed service/runtime operations remain authoritative. Browser state
is a cache, projection, and UI coordination layer.

## Starting Point

- Read ADR 0010: `docs/adr/0010-flight-deck-local-graph-store.md`.
- Start from the merged `grm-flight-deck` React/Vite app.
- Preserve the existing fixture/service snapshot behavior.
- Keep the first slice TypeScript/browser-only.
- Leave future in-memory `grm-rs` via WASM or sidecar as a later
  implementation behind the same store boundary.

## Scope

### 1. Store Shape

Create a small graph-store module for:

- connection profiles;
- selected profile ID;
- connection status and last error;
- workspace ID and snapshot limit;
- schema models and schema edges;
- nodes by stable string ID;
- edges by stable string ID;
- selected node/edge;
- graph filter/delimiter state;
- graph layout/view state; and
- execution-event buffer.

Expose GRM-shaped actions such as:

- `saveConnectionProfile(profile)`;
- `selectConnectionProfile(profileId)`;
- `loadSnapshot(snapshot)`;
- `applyGraphFilter(filter)`;
- `selectNode(id)`;
- `selectEdge(id)`;
- `applyExecutionEvent(event)`;
- `clearWorkspace()`.

Use Zustand if the dependency and style fit the app. A local reducer/context is
acceptable if it keeps the first slice smaller. Do not build a generic graph
state framework.

### 2. Profile Persistence

Persist only safe profile details in browser storage:

- profile name;
- gateway/service base URL;
- workspace ID;
- connection mode;
- fixture mode;
- snapshot limit;
- last selected profile ID; and
- optional explicit auto-load flag.

Restore the last selected profile when the page loads. Show the restored
profile in the UI before making any network request. Auto-load only when the
profile explicitly permits it.

Migrate from the existing single connection-settings storage key if needed.

### 3. Snapshot Normalization

Normalize loaded snapshots into maps:

- `nodesById`;
- `edgesById`;
- model indexes where useful;
- derived visible graph from current filter/delimiter.

Keep IDs as strings end to end in the UI.

### 4. UI Integration

Move current connection, snapshot, filter, selection, and event state out of
`App.tsx` into the store where it belongs.

Keep component props focused on rendering and user interaction. Do not let the
store absorb Cytoscape instance lifecycle or DOM-only concerns.

## Non-Goals

- No Rust/WASM integration.
- No browser-side canonical GRM runtime.
- No new public query language.
- No production authentication or authorization.
- No raw credential, raw certificate, private key, bearer token, password,
  private filesystem path, policy table, or unbounded request/response-body
  persistence.
- No 3D/spatial UI work.

## Constraints

- The store is a browser cache/projection, not the source of truth.
- Service calls still go through typed GRM service/gateway requests.
- Persisted local data must be versioned and migration-safe.
- UI copy must be honest about fixture data, local anonymous dev mode, and
  deferred secured profiles.
- Tests should sit at the store/API/component boundary, not private runtime
  internals.

## Acceptance Criteria

- A local graph-store module exists with typed state and actions.
- Returning users see the last selected safe connection profile restored.
- Existing single-profile local storage is migrated or safely ignored with a
  clear default.
- Snapshot loading normalizes nodes and edges by string ID.
- The graph filter derives a visible graph from store state.
- Selection survives harmless UI rerenders and clears when selected data is no
  longer present.
- No secret-like fields are persisted.
- Existing fixture and service snapshot flows still work.
- The app builds successfully.

## Expected Checks

- `npm run build` in `grm-flight-deck`.
- Store unit tests for persistence, migration, normalization, filtering, and
  secret redaction.
- Component or integration tests for restored profile and basic snapshot load
  behavior if the test harness exists or is added in this slice.

