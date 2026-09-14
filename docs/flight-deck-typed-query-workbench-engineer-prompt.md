# GRM Flight-Deck Typed Query Workbench Engineer Prompt

## Goal

Implement the next flight-deck slice: a typed query workbench with explicit
Data and Schema modes, a small read-only GRM syntax adapter for data queries,
and real service-backed query/explain/profile execution where the existing
typed service path supports it.

This is a mechanism-proof slice for the cockpit. The user should be able to
type a familiar GRM command such as `node.find WorkSlice status=active limit=25`,
execute it from the Query panel, see graph results, and inspect explain/profile
evidence without the browser or gateway treating text as the canonical service
contract.

## Why This Matters

The flight-deck now has the first usable service-backed UI, a safe local graph
store, connection profiles, read-only security status, and a query/audit
workbench shape. The next useful product move is to make the Query panel
operate on typed GRM requests rather than only filtering a loaded snapshot.

This strengthens GRM's core promise: typed, explainable operational memory that
is usable by humans and ready for later MCP-agent assistance.

## Project Memory Context

Use GRM project memory before editing.

Inspect at least:

- RoadmapItem `13`, "Build the GRM flight-deck"
- WorkSlice `573`, "Implement flight-deck local graph store and connection profiles"
- WorkSlice `584`, "Add read-only service identity and security-status surface"
- Constraint `4`, "Structured operations are canonical"
- Policy `54`, "Tests reinforce code ownership boundaries"
- Decision `107`, "Frame GRM as Structured Operational Memory Layer"
- Decision `138`, "Service-backed workspace is the default persisted SOML path"
- Decision `561`, "Choose 2D-first TypeScript browser flight-deck with event-stream animation path"
- Decision `570`, "Use a GRM-shaped local graph store for the flight-deck"
- Risk `587`, "Admin-plane mutation could sneak in through bootstrap ergonomics"

Implications:

- Text syntax is an adapter over typed requests, not the service contract.
- Product-facing behavior belongs in frontend, gateway, service, or runtime
  tests at the boundary users actually call.
- The browser store remains a cache/projection over service/runtime truth.
- Query/explain/profile claims must distinguish real service evidence from
  local UI summaries.
- Admin-plane mutation remains out of scope.

## Read First

Start in `/home/laurie/source/grm-rs`.

Read:

- `docs/adr/0010-flight-deck-local-graph-store.md`
- `docs/flight-deck-query-audit-workbench-engineer-prompt.md`
- `docs/flight-deck-query-language-thinking.md`
- `grm-flight-deck/README.md`
- `grm-flight-deck/src/App.tsx`
- `grm-flight-deck/src/GraphCanvas.tsx`
- `grm-flight-deck/src/api.ts`
- `grm-flight-deck/src/graphStore.ts`
- `grm-flight-deck/src/types.ts`
- `grm-flight-deck/tests/graphStore.test.ts`
- `grm-flight-deck-gateway/src/main.rs`
- `src/runtime/parser.rs`
- `src/runtime/ops.rs`
- `src/runtime/session.rs`
- `grm-service-api/src/lib.rs`
- `grm-service-api/proto/grm/service/v1/query.proto`
- `grm-service-api/proto/grm/service/v1/workspace.proto`

Treat the existing query/audit workbench and local explain/profile panels as
implementation truth, but not as final product truth: today they are local
summaries unless real service-backed query/explain/profile is added.

## Scope

### 1. Data And Schema Query Modes

Make the Query panel's mode explicit:

- Data mode operates on graph instances.
- Schema mode operates on node models, edge models, fields, and legal edge
  directions.

The mode should be shared by the query controls and the graph display. Avoid a
state split where `GraphCanvas` shows Schema while the query controls still
behave like Data.

Data mode should be the default.

### 2. Read-Only GRM Syntax Adapter

Add a small read-only command input for Data mode.

Support a deliberately narrow subset first:

```text
node.find <Model> [field=value ...] [via=<out|in|both>:<EdgeModel>:<EndModel> ...] [end.<field>=value ...] [edge.<field>=value ...] [return=root|end] [order=<field>:asc|desc[,<field>:asc|desc ...]] [limit=<n>] [offset=<n>]
edge.find <EdgeModel> [from=<id>] [to=<id>] [field=value ...] [order=<field>:asc|desc[,<field>:asc|desc ...]] [limit=<n>] [offset=<n>]
session.explain node.find <Model> ...
session.explain edge.find <EdgeModel> ...
session.profile node.find <Model> ...
session.profile edge.find <EdgeModel> ...
```

Use the existing runtime parser/adapter behavior where practical, or build a
small parser that compiles to the same typed request structures. Do not send
free-form command text as the service contract.

Reject writes and unsupported commands clearly. At minimum reject:

- `schema.define`
- `node.create`
- `node.update`
- `node.delete`
- `edge.create`
- `edge.update`
- `edge.delete`
- `batch`
- `session.import`
- `session.load`
- `session.save`
- any Cypher-like `MATCH`, `CREATE`, `MERGE`, `SET`, or `DELETE`

For this slice, defer `return=edge` if the current `GrpcWorkspaceClient`
ergonomic path still rejects it.

### 3. Typed Gateway Execution

Add focused read-only gateway endpoints only if needed.

Preferred shape:

- browser sends a typed query request shape or a command that the gateway
  parses into a typed request before calling the service;
- gateway calls the workspace-scoped typed service path through
  `GrpcWorkspaceClient`;
- gateway returns bounded, graph-displayable results plus explain/profile
  metadata where requested.

Keep the gateway as a local read-only adapter over the service. Do not add
admin, policy, permission, principal, credential, certificate, or workspace
mutation endpoints.

Gateway errors must remain redacted. Do not log local cert/key paths, raw
credential-bearing URLs, policy tables, fingerprints, or unbounded request
payloads.

### 4. Graph Results

For successful data queries:

- replace or update the visible graph result with the bounded query result;
- preserve selection inspection;
- record a bounded read event in the local event buffer;
- show omitted/partial indicators when limits or gateway response shape require
  them.

If a query returns only edges, include known endpoints if available from the
current snapshot or service response. If endpoints are not available, show an
honest unsupported or partial state instead of drawing incoherent graph data.

### 5. Explain And Profile

For `session.explain ...` and `session.profile ...`, use real service/runtime
responses when the gateway path supports them.

The UI must label evidence accurately:

- real service/runtime explain/profile as service evidence;
- client-side summaries as local summaries;
- unsupported query shapes as unsupported.

Explain/Profile panels should stay alongside the Query graph output. Do not
move them into Audit.

### 6. Schema Mode

Schema mode does not need the same text syntax in this slice.

Implement a small typed schema inspection experience using existing snapshot
schema data:

- filter node models and edge models by name;
- inspect model fields if field data is available through the service/gateway;
- show legal edge directions from `schemaEdges`;
- keep unsupported field/detail states honest.

If field data is not currently returned by the gateway snapshot, either extend
the read-only snapshot response to include bounded schema fields or leave field
inspection as a clearly marked follow-up.

### 7. Documentation

Update flight-deck docs just enough to describe:

- Data versus Schema mode;
- supported read-only GRM syntax subset;
- unsupported write commands;
- Cypher deferred as a future read-only adapter;
- text syntax as an adapter over typed GRM requests;
- whether explain/profile data is real service evidence or local summary.

## Non-Goals

- No Cypher support in this slice.
- No general textual query language as a service contract.
- No writes from the browser query command bar.
- No admin plane.
- No live user, principal, access-level, role, or permission mutation APIs.
- No policy editor or permission-table viewer.
- No browser handling of private keys, client certificates, passwords, bearer
  tokens, raw fingerprints, local filesystem paths, or credential-bearing URLs.
- No external/high-assurance audit forwarding.
- No attestation, signed receipts, state commitments, or autonomous correctness
  claims.
- No MCP agent chat yet; this slice should make that later work easier.
- No 3D/spatial UI work.

## Constraints

- Preserve `Constraint 4`: structured operations are canonical.
- Preserve `Policy 54`: test through the public surface that owns the behavior.
- Preserve `Decision 107`: GRM is SOML, not mainly a graph CRUD screen or
  Cypher clone.
- Preserve `Decision 138`: service-backed workspace remains the default
  persisted SOML path.
- Preserve `Decision 570`: the SPA graph store is a browser projection, not
  canonical runtime truth.
- Keep secured-local additive and keep anonymous/docker-local-insecure visibly
  weaker than secured.
- Keep all returned graph/query/audit data bounded.
- Keep unsupported states explicit rather than silently widening claims.

## Suggested Implementation Shape

Prefer a small sequence:

1. Add frontend types for query mode, command text, query status, query result
   provenance, and explain/profile provenance.
2. Move any cross-view query state into `graphStore.ts`.
3. Add a minimal parser/validator boundary, preferably in the gateway if it can
   reuse Rust runtime parsing without exposing command text as the service
   contract.
4. Add typed gateway execution for `node.find`, `edge.find`,
   `session.explain`, and `session.profile`.
5. Render results in the existing `GraphCanvas`/Query panel shape.
6. Update docs and focused tests.

If reusing Rust CLI parser internals would require broad visibility changes,
keep the first parser local and small, but ensure its output is typed request
objects and its unsupported diagnostics are tested.

## Acceptance Criteria

- Query opens in Data mode by default.
- Data and Schema modes are visible and control both query controls and graph
  presentation.
- Data mode accepts the supported read-only GRM syntax subset.
- Write-like and Cypher-like commands are rejected before any service call.
- Supported data commands compile to typed GRM query/explain/profile requests.
- Successful `node.find` and `edge.find` commands return bounded graph results.
- `session.explain` and `session.profile` display real service/runtime evidence
  where implemented, or an explicit unsupported state where not.
- Schema mode supports model/edge inspection without pretending to be data
  traversal.
- Query execution records bounded local read events.
- No secret-like values are persisted, displayed, or logged.
- Fixture and service snapshot flows still work.
- Docs state the implemented syntax and truth boundaries.

## Expected Tests

Frontend:

```bash
npm run test
npm run build
```

Run these from `grm-flight-deck`.

Add or update tests for:

- Data/Schema mode state;
- command text state and reset behavior;
- read-only command acceptance and write-command rejection;
- visible graph update after query execution;
- explain/profile provenance state;
- event recording for query execution;
- non-persistence of command history if it could contain sensitive literals.

Gateway/service:

```bash
cargo test -p grm-flight-deck-gateway
```

Also run:

```bash
cargo test -p grm-service-api
```

only if service/API code changes.

Runtime:

```bash
cargo test --test runtime_session
```

only if parser, runtime request conversion, or explain/profile runtime behavior
changes.

Before handoff, run the narrowest useful clippy target covering touched Rust
crates, for example:

```bash
cargo clippy -p grm-flight-deck-gateway --all-targets -- -D warnings
```

If service/API or runtime crates are touched, extend clippy accordingly.

## Handoff Notes

In the final engineer report, include:

- the exact supported command subset;
- where text is parsed into typed requests;
- which explain/profile data is real service evidence versus local summary;
- how write/Cypher commands are rejected;
- what changed in Data versus Schema mode;
- which tests passed;
- any unsupported query shapes intentionally deferred.

After merge or a clear implementation decision, update project memory:

- create or update a WorkSlice under RoadmapItem `13` for this typed query
  workbench slice;
- link relevant constraints/decisions/policies;
- keep Risk `587` active unless a separate admin-boundary design resolves it;
- add a follow-up WorkSlice for MCP agent chat over typed query context.
