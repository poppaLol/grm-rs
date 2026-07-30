# GRM Flight-Deck First UI Engineer Prompt

## Goal

Promote the `spike/react-vite-flight-deck` experiment into the first real GRM
flight-deck UI branch for the service.

Build a small, honest workbench that connects to a GRM service, shows the
current graph/schema context, and lets a user narrow the visible graph through a
query bar. Keep the UI 2D-first and service-backed. Preserve the spatial and
execution-animation ideas as follow-on layers, not as blockers for the first
usable surface.

## Why This Matters

The flight-deck is the first human-facing operational surface over GRM. It
should make the service understandable without changing the canonical boundary:
GRM operations remain typed structured requests and responses. The UI may offer
text input, graph gestures, filters, and visual affordances, but those are
adapter ergonomics over structured service calls.

## Starting Point

- Use branch `explore/flight-deck-service-ui` as the target branch.
- Use `spike/react-vite-flight-deck` as the implementation seed unless a fresh
  comparison explicitly selects Vue/Vite before coding starts.
- Preserve the Bevy, Leptos/Cytoscape, egui, and collected reference branches as
  evidence and fallback material.
- Do not pull all spike artifacts forward wholesale. Promote only the pieces
  needed for the first service-backed workbench.

## Scope

### 1. Service Connection Details

Add a connection setup surface for the GRM service:

- service base URL;
- connection mode, starting with local anonymous/dev mode;
- visible connection status and last error;
- workspace/session target where the service API requires one;
- configuration persistence appropriate for a local development UI.

Extend the design for secured connections without overclaiming browser support:

- browser JavaScript cannot directly choose and attach arbitrary client TLS
  certificates to `fetch`;
- mTLS should be handled by browser-managed certificates, a local connector,
  service-side profiles, or a reverse-proxy/dev helper;
- certificate/key file selection can be prototyped as local profile input only
  if the backend path that consumes it is explicit and safe.

### 2. 2D Force Graph

Add a 2D graph inspection view:

- render nodes and edges from service data;
- support pan, zoom, fit-to-view, hover, and selection;
- show selected node/edge details in a side panel;
- keep graph library usage replaceable behind a small adapter component;
- prefer proven browser graph libraries for layout and interaction.

### 3. Query Bar To Delimit The Graph

Add a query/filter bar that narrows the displayed graph:

- start with a simple, honest subset such as label/type filters, id search,
  property key/value search, or saved sample queries;
- translate UI input into typed service requests where supported;
- show empty, loading, error, and partial-result states;
- do not imply a stable public query language unless one is already accepted and
  implemented.

### 4. Event-Stream Animation Hook

Leave a narrow extension point for execution animation:

- define a UI-side event model for reads, writes, node creation, and edge
  traversal;
- allow mock events in development;
- do not make real-time animation part of first-slice acceptance unless the
  service already exposes a suitable event stream.

## Non-Goals

- Do not build a full admin console.
- Do not add policy authoring, user management, or hosted control-plane flows.
- Do not make 3D/spatial navigation part of the first deliverable.
- Do not introduce a new canonical query language through the UI.
- Do not weaken service security boundaries for local convenience.

## Constraints

- The UI is an adapter over typed GRM service/runtime operations.
- Claims in docs and UI copy must match implemented behavior.
- Security context and audit views must avoid raw credentials, private paths,
  raw certificates, policy table contents, and unbounded request/response
  bodies.
- Keep the first slice small enough to review and demo.

## Acceptance Criteria

- A React/Vite/TypeScript flight-deck app exists on the exploration branch.
- The app can configure and attempt a local GRM service connection.
- Connection status, success, and failure are visible to the user.
- A 2D graph view renders service-backed or clearly labeled fixture-backed graph
  data.
- A query/filter bar changes the displayed graph boundary.
- Selected node/edge details are inspectable.
- The implementation has a clear place to receive future execution events.
- Documentation states what is real, what is fixture/demo data, and what remains
  deferred.

## Expected Checks

- Run the frontend typecheck/build command.
- Run any added unit/component tests.
- If a dev server is used, verify the primary screen manually in a browser or
  with Playwright screenshots.
- If the UI calls the service, run a local service smoke path against the public
  service surface rather than private internals.

