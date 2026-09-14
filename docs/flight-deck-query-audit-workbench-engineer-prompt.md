# GRM Flight-Deck Query/Audit Workbench Engineer Prompt

## Goal

Turn the current flight-deck UI sketch into a focused, tested workbench slice
for query, explain, profile, and audit navigation.

The user should normally live in the query arena: execute a bounded graph read,
inspect the graph output, and optionally display explain/profile context in the
same view. Audit should be a distinct navigable panel, not a footer attached to
the graph. Connection details should stay out of the way unless the user wants
to connect or change connection.

## Why This Matters

The flight-deck has now proven the current user principal through the
read-only security-status path and Admin no.1 onboarding. The next product
step is making the cockpit feel like an operational workbench rather than a
connection form with a graph below it.

This slice improves daily usability and sets the shape for real service-backed
query/explain/profile/audit operations without broadening GRM's service
contract or slipping into administration.

## Starting Point

- Work in `/home/laurie/source/grm-rs`.
- Keep the existing code changes in:
  - `grm-flight-deck/src/App.tsx`
  - `grm-flight-deck/src/style.css`
- Treat those changes as a prototype starting point, not as final design.
- Read the relevant context before editing:
  - `docs/adr/0010-flight-deck-local-graph-store.md`
  - `docs/flight-deck-graph-store-engineer-prompt.md`
  - `docs/flight-deck-security-audit-observability-engineer-prompt.md`
  - `docs/secured-local-admin-1-flight-deck-onboarding.md`
  - `grm-flight-deck/README.md`
  - `grm-flight-deck/src/App.tsx`
  - `grm-flight-deck/src/graphStore.ts`
  - `grm-flight-deck/src/api.ts`
  - `grm-flight-deck-gateway/src/main.rs`
- Treat WorkSlice 584 as completed implementation truth: flight-deck can show
  safe read-only identity/security status.
- Treat Risk 587 as active: admin-plane mutation must not sneak in through
  bootstrap, connection, query, audit, or profile ergonomics.

## Scope

### 1. Connection Chrome

Keep connection details hidden by default once a profile/workspace is selected.

The default header should show only a compact, non-secret summary:

- selected profile name;
- connection/profile kind, such as fixture, anonymous local, docker local
  insecure, or secured;
- workspace name; and
- current identity/security badge when available.

Expose the editable connection/profile fields only through an explicit
connect/change action. Do not persist or display secret-like fields.

### 2. Query Workbench

Make Query the default and primary workspace panel.

The query view should include:

- graph filter/query controls;
- an explicit execute action;
- the graph output arena;
- selection inspection;
- optional explain display; and
- optional profile display.

For this slice, it is acceptable to keep the existing bounded snapshot/filter
operation as the executable query if a narrower real gateway query endpoint is
not already ready. If adding a gateway/service operation, keep it over typed
structured GRM requests and reuse existing runtime/service behavior.

Do not introduce a new textual query language as the canonical service
contract.

### 3. Explain And Profile

Explain and profile should be displayable alongside the graph output in the
same Query view.

Prefer real typed explain/profile data if it is already available through the
current service/gateway path. If it is not, use honest UI-side derived
summaries from the current snapshot/filter and label them as local view
summaries in code/tests/docs as needed.

The UI must not imply that client-side summaries are service planner truth.

Useful explain/profile fields may include:

- operation family;
- workspace;
- model scope;
- predicates or filters;
- result shape;
- source rows versus visible rows;
- configured limit;
- omitted/partial result indicators; and
- service/gateway capability or unsupported state.

### 4. Audit Navigation

Make Audit a first-class navigable workspace panel.

The audit panel should be visually and structurally distinct from the query
panel. It may initially use the existing execution-event buffer and fixture
events, but it should be shaped so a later read-only audit gateway can feed it
without redesign.

The audit panel should show bounded, redacted, operator-useful entries such as:

- event kind;
- observed/pending/fixture status;
- operation summary;
- principal/security context when already safe and available; and
- result/outcome state.

### 5. Store And Component Boundaries

Move persistent or cross-view UI state into `graphStore.ts` when it belongs
there, especially:

- selected workspace panel;
- connection tray open/closed state if worth preserving;
- explain/profile visibility;
- last executed query context; and
- audit/event buffer updates.

Keep DOM-only and Cytoscape lifecycle state inside components.

### 6. Documentation

Update flight-deck docs just enough to explain the new workbench shape:

- connection details are hidden by default;
- Query is the normal graph-output arena;
- Explain/Profile can be shown alongside Query;
- Audit is navigable and bounded; and
- any explain/profile/audit data that is still local/derived/fixture-only is
  clearly described.

## Non-Goals

- No live service-side user, principal, access-level, role, or permission
  mutation APIs.
- No admin plane.
- No policy editor.
- No permission table viewer.
- No browser private-key, client-certificate, password, bearer-token, raw
  credential, certificate fingerprint, or local filesystem path handling.
- No production or hosted identity lifecycle.
- No external/high-assurance audit forwarding.
- No attestation, signed receipts, or state commitments.
- No new canonical textual query language.
- No broad redesign of the gateway beyond what the typed workbench operation
  requires.
- No 3D/spatial UI work.

## Constraints

- Preserve `Constraint 4`: structured operations are canonical.
- Preserve `Policy 54`: product-facing behavior should be tested through the
  public surface users call.
- Preserve `Decision 107`: GRM is a Structured Operational Memory Layer, not
  primarily a graph CRUD screen or query-language demo.
- Keep anonymous_local and docker_local_insecure visibly weaker than secured.
- Keep secured-local additive; it must not become the default startup profile.
- Keep the browser store a cache/projection over service/runtime truth.
- Keep security claims limited to implemented, tested local behavior.
- Keep fixture/local-derived explain/profile/audit clearly distinct from real
  service evidence.

## Acceptance Criteria

- The app opens with Query as the main arena.
- Connection fields are hidden by default and can be shown explicitly.
- The compact connection summary remains readable and non-secret.
- Query execution is explicit and records a bounded event.
- Explain and Profile can be shown alongside the graph output without leaving
  the query view.
- Audit is reachable as its own navigable panel and is not rendered as a graph
  footer.
- Selection inspection still works.
- Fixture and service snapshot flows still work.
- No secret-like fields are persisted, displayed, or logged.
- The UI does not claim service-backed explain/profile/audit truth unless the
  data really comes from that surface.
- Documentation and tests reflect implemented truth.

## Expected Checks

- `npm run test` in `grm-flight-deck`.
- `npm run build` in `grm-flight-deck`.
- Add or update frontend/store tests for:
  - selected workspace panel;
  - connection detail visibility/default;
  - explain/profile visibility;
  - execution event recording; and
  - non-persistence of secret-like connection fields.
- If the gateway/service changes, also run focused Rust checks:
  - `cargo test -p grm-flight-deck-gateway`
  - `cargo test -p grm-service-api` when service/API code changes.

## Skills To Use

- `grm-product-manager`
- `grm-project-memory`
- `grm-project-constraints`
- `grm-security-engineer` if touching security status, audit, redaction,
  secured-local behavior, or gateway/service security surfaces.
- `grm-architect` if introducing or changing service/gateway boundaries for
  typed query/explain/profile/audit data.

## PM Review After Engineering

Return to `grm-product-manager` after the engineering slice for review.

The PM review should check:

- whether the slice stayed inside query/audit workbench ergonomics;
- whether typed service/runtime boundaries remained intact;
- whether explain/profile/audit claims match implementation truth;
- whether Risk 587 remains active or needs updated wording;
- whether a new work slice should be recorded for real service-backed
  query/explain/profile/audit data; and
- whether docs and graph memory need updates after merge.

