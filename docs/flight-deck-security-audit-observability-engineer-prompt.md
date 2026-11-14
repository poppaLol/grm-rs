# GRM Flight-Deck Read-Only Audit Observability Engineer Prompt

## Goal

Implement WorkSlice 602: add real read-only security/audit observability to the
flight-deck.

The flight-deck already has a Query/Audit workbench and read-only
`/api/security/status`, but the Audit panel is still backed by a bounded
browser-side event buffer. This slice should connect the flight-deck to
service-authored audit truth through a narrow, redacted, read-only service and
gateway path.

The product phrase is: **observability before administration**.

## Why This Matters

The lower-friction Admin no.1 onboarding flow is now in place: port-aware
secured-local bootstrap, generated `gateway.env`, one-command local connector
startup, safe profile metadata, and clearer Gateway URL language.

The remaining adoption gap is trust legibility. After Admin no.1 connects, the
operator should not need to inspect terminal logs or generated files to answer:

- who they are;
- which local security profile is active;
- whether mandatory secured audit is healthy;
- whether recent service-authored audit evidence exists; and
- what bounded, redacted security outcomes were recently recorded.

This is not an admin plane. It is a compact read-only confidence layer over
implemented local service/security evidence.

## Current Position

- RoadmapItem 13, `Build the GRM flight-deck`, is active.
- RoadmapItem 248, `Define and implement security core objects`, is active.
- WorkSlice 602, `Add read-only flight-deck security and audit observability`,
  is `next`.
- WorkSlice 584, read-only service identity/security status, is completed.
- WorkSlice 585, secured-local connector simplification, is completed.
- Risk 587, `Admin-plane mutation could sneak in through bootstrap ergonomics`,
  remains active and must stay active unless a separate accepted admin-boundary
  design exists.

Implementation truth:

- `grm-flight-deck/README.md` says the Audit panel is currently backed by a
  bounded UI event buffer, not a service audit-event read path.
- `docs/security/security-audit.md` describes the bounded, redacted,
  service-authored audit model and flight-deck direction.
- `docs/adr/0009-grm-native-audit-log-store.md` accepts the service-owned local
  audit log store as the source-of-truth event list for local service audit.
- `grm-service-api/proto/grm/service/v1/service.proto` currently exposes
  `SecurityStatus`, but not a flight-deck audit status/events RPC.
- `grm-flight-deck-gateway/src/main.rs` currently exposes
  `/api/security/status` and redacts gateway errors.
- `grm-flight-deck/src/api.ts` currently fetches `/api/security/status`, but
  has no service audit status/event fetch path.

## Starting Point

- Work in `/home/laurie/source/grm-rs`.
- Read the current security and flight-deck context before editing:
  - `docs/adr/0008-bounded-authoritative-security-audit.md`
  - `docs/adr/0009-grm-native-audit-log-store.md`
  - `docs/adr/0011-passwordless-secured-local-bootstrap.md`
  - `docs/security/security-audit.md`
  - `docs/security/security-design.md`
  - `docs/security/threat-model.md`
  - `docs/secured-local-admin-1-flight-deck-onboarding.md`
  - `grm-flight-deck/README.md`
  - `grm-service-api/proto/grm/service/v1/service.proto`
  - `grm-service-api/src/lib.rs`
  - `grm-service-api/src/audit_store.rs`
  - `grm-service-api/tests/security_workspace.rs`
  - `grm-service-api/tests/durable_audit_store.rs`
  - `grm-flight-deck-gateway/src/main.rs`
  - `grm-flight-deck/src/App.tsx`
  - `grm-flight-deck/src/api.ts`
  - `grm-flight-deck/src/types.ts`
  - `grm-flight-deck/src/fixtures.ts`
  - `grm-flight-deck/src/graphStore.ts`
  - `grm-flight-deck/tests/graphStore.test.ts`
- Add browser-level reliability coverage as part of this slice. Prefer
  Playwright unless Cypress is already selected by the implementation branch.
  The current flight-deck package has a small Node/esbuild store test harness
  but no browser E2E baseline.
- Use these skills:
  - `grm-product-manager`
  - `grm-security-engineer`
  - `grm-project-memory`
  - `grm-project-constraints`

## Scope

### 1. Add A Narrow Read-Only Service Surface

Add the smallest typed service API needed for the flight-deck to inspect audit
observability.

Prefer a status-first shape unless the existing service/audit store already has
a safe event-list path that can be exposed narrowly:

```text
SecurityAuditStatus(SecurityAuditStatusRequest)
```

or a similarly named read-only RPC. The public gateway should expose it as:

```text
GET /api/security/audit/status
```

Only add a bounded recent-events path if it can be implemented without creating
a broad audit query API:

```text
GET /api/security/audit/events?limit=N
```

If recent events are included, make them a bounded field inside the status
response or a narrow fixed-shape endpoint. Do not create a general audit search
language, SQL-ish filter language, policy viewer, or admin API.

Useful service-side fields include:

- security profile;
- audit mode: mandatory, best-effort, unavailable, or not applicable;
- audit sink health: healthy, degraded, unavailable, unknown;
- whether mandatory secured audit is currently available;
- retained event count, if already available safely;
- recent event count, bounded by caller-independent limits;
- retention limit summary, if already modeled safely;
- last recovery/degradation status code, if already modeled safely; and
- bounded recent event summaries.

Useful recent event summary fields include:

- timestamp if already present and safe;
- request ID or correlation ID only if bounded and non-sensitive;
- service sequence if safe and useful;
- stage: attempt, authentication, authorization, admission, runtime,
  durability, delivery;
- decision and bounded reason code;
- authenticated principal issuer/subject, when already stored safely;
- authentication method;
- policy version;
- operation family or action;
- workspace reference only when already opaque and safe;
- runtime outcome;
- durability outcome; and
- delivery outcome.

Use existing audit store and service security types wherever possible. Do not
invent a parallel audit model in the gateway or browser.

### 2. Preserve Authentication, Authorization, And Audit Semantics

The read path is read-only, but it still crosses the service boundary.

Inspect the existing security action/permission taxonomy before selecting the
service-side authorization shape. If there is already a suitable status or
security-observability action, reuse it. If a new action is needed, keep it
narrow and document why it is not administration.

The implementation must not:

- let the browser submit effective identity, permission, policy decision, or
  audit outcome;
- derive security truth from the gateway or UI event buffer;
- bypass the canonical service enforcement path;
- turn mTLS transport identity directly into authorization;
- expose missing workspace or handle existence before authorization; or
- weaken mandatory secured audit fail-closed behavior.

Anonymous-local and Docker-local-insecure may report best-effort or unavailable
audit observability, but they must never masquerade as secured mandatory audit.

### 3. Add A Gateway JSON Adapter

Extend `grm-flight-deck-gateway` with a redacted read-only endpoint:

```text
GET /api/security/audit/status
```

The gateway should:

- use the same upstream endpoint and TLS environment handling as the existing
  security-status and query paths;
- translate typed service responses into compact browser-safe JSON;
- clamp any request limit to a small maximum if recent events are returned;
- return stable non-sensitive errors; and
- log only redacted failure messages.

Do not expose:

- raw certificates;
- private keys;
- cert/key paths;
- certificate fingerprints;
- rendered permission tables;
- policy internals;
- graph property values;
- local filesystem paths;
- unbounded audit payloads;
- bearer tokens or passwords; or
- arbitrary upstream error text.

### 4. Update The Flight-Deck UI

Extend the existing security/status and Audit workbench so users can distinguish
local UI events from service-authored audit evidence.

The UI should make these states legible:

- fixture;
- anonymous local;
- Docker local insecure;
- secured and healthy;
- secured and degraded/unavailable;
- gateway/service unavailable; and
- audit observability unsupported by the current profile.

Expected shape:

- The header or security context area continues to show identity/security
  status.
- The Audit panel becomes the home for service audit observability plus the
  existing bounded local event buffer.
- Service-authored audit status/event summaries should be visually distinct
  from local UI/query events.
- Fixture mode should include realistic, safe fixture audit status and event
  summaries.
- Empty or unsupported audit states should be explicit and calm, not treated as
  successful secured evidence.

Keep the UI compact and operational. Do not build an admin wizard.

### 5. Documentation

Update docs just enough to describe implemented truth:

- `grm-flight-deck/README.md`
- `docs/secured-local-admin-1-flight-deck-onboarding.md`
- `docs/security/security-audit.md` if the public read surface changes audit
  observability semantics

The docs should say what the flight-deck now reads from the service, what
remains local UI event history, and what is still not implemented.

### 6. Add A Browser Reliability Baseline

Before moving on to visual projection/schema work, add a small browser-level
reliability test suite that shows where the flight-deck stands today.

Prefer Playwright for this repository unless the active implementation branch
has already adopted Cypress. Keep the suite small, deterministic, and useful
for branch handoff rather than trying to exhaustively test the browser app.

Minimum useful Playwright/Cypress coverage:

- fixture-mode flight-deck loads and renders the main workbench;
- graph/schema/query areas render without uncaught browser errors;
- security status is visible in fixture mode and clearly labeled as fixture or
  non-service evidence;
- Audit panel distinguishes service-authored audit evidence from local UI/query
  event history using fixture data;
- gateway/service unavailable state is visible and does not look like secured
  audit success;
- local browser storage does not persist obvious secret-like fields after the
  fixture and unavailable flows;
- at least one test captures a screenshot or trace artifact suitable for
  reliability review in CI or local handoff.

If a full secured-local service/gateway stack is too heavy for the first browser
baseline, do not block the slice on it. Add fixture and mocked/unavailable
gateway flows first, then document the follow-up needed for an optional
secured-local E2E smoke.

Recommended package shape if Playwright is selected:

```text
grm-flight-deck/playwright.config.ts
grm-flight-deck/tests/e2e/*.spec.ts
```

Recommended scripts:

```json
"e2e": "playwright test",
"e2e:headed": "playwright test --headed"
```

The reliability baseline is not a substitute for service/gateway public-boundary
tests. It answers a different question: can a user open the flight-deck and see
the expected security/audit states without a broken or misleading UI?

## Non-Goals

- No live service-side user, principal, access-level, role, or permission
  mutation APIs.
- No admin plane.
- No policy editor.
- No permission-table viewer.
- No browser private-key, client-certificate, bearer-token, password, raw
  credential, or local credential-path handling.
- No production or hosted identity lifecycle.
- No external or high-assurance audit forwarding.
- No general audit query/search language.
- No SIEM-style ingestion/export.
- No attestation, signed receipts, state commitments, client-verifiable
  continuity, non-repudiation, or tamper-evident log claim.
- No hosted durability, multi-writer coordination, or hosted security claim.
- No 3D/spatial work.
- No visual projection/schema implementation in this slice. Add the browser
  reliability baseline first so the next visual work has a trustworthy UI
  footing.

## Constraints

- Preserve Constraint 4: structured operations are canonical.
- Preserve Policy 54: product-facing behavior is tested through the public
  surface users call.
- Preserve Decision 107: GRM is a Structured Operational Memory Layer, not
  primarily a graph CRUD screen or query-language demo.
- Preserve accepted security decisions:
  - transport identity is distinct from application identity;
  - actor identifiers are assertions unless delegated;
  - secured service policy defaults to deny;
  - local insecure and anonymous modes are explicitly weaker;
  - security claims require implementation and public tests; and
  - bounded redacted audit distinguishes authentication, authorization,
    admission, runtime, durability, and delivery evidence.
- Keep Risk 587 active. This slice may reduce user confusion, but it does not
  resolve the future admin-boundary risk.
- Keep security claims limited to tested local secured behavior.
- Keep the audit read model bounded, redacted, and service-authored.
- Prefer existing audit store, security status, and gateway redaction helpers
  over new parallel concepts.

## Acceptance Criteria

- A real service-backed read-only audit status path exists and is available
  through the local flight-deck gateway.
- Flight-deck shows current security profile, principal, authentication method,
  policy version, audit availability/status, and any bounded recent safe audit
  activity together for the Admin no.1 secured-local path.
- The Audit panel clearly separates service-authored audit evidence from local
  UI/query event history.
- Anonymous local, Docker local insecure, fixture, secured, unavailable, and
  error states are explicit and cannot masquerade as secured audit evidence.
- Browser responses and normal gateway logs do not expose raw certificates,
  private keys, cert/key paths, certificate fingerprints, permission tables,
  policy internals, local filesystem paths, bearer tokens, passwords, raw graph
  values, or unbounded audit payloads.
- Secured mandatory audit unavailability remains fail-closed for protected
  effects according to the existing service rules.
- The implementation remains read-only.
- Documentation describes the implemented truth and preserves non-goals.
- A small Playwright or Cypress browser reliability baseline exists and can show
  fixture, unavailable, and audit/security legibility states without requiring a
  full secured-local stack.
- Browser reliability artifacts such as screenshots or traces are available
  from at least one test run for handoff/debugging.

## Expected Checks

Run the relevant subset during development, then the full set before handoff
when feasible:

```bash
cargo test -p grm-service-api
cargo test -p grm-flight-deck-gateway
cargo clippy -p grm-service-api --all-targets -- -D warnings
cargo clippy -p grm-flight-deck-gateway --all-targets -- -D warnings
```

From `grm-flight-deck`:

```bash
npm run test
npm run build
npm run e2e
```

Add or update focused tests for:

- service/API safe audit status serialization;
- secured mandatory audit healthy/degraded/unavailable states;
- anonymous-local and Docker-local-insecure weaker audit states;
- bounded recent event summaries if exposed;
- redaction of paths, credentials, policy internals, graph values, and raw
  upstream errors;
- gateway endpoint success/error behavior;
- fixture and real-service UI rendering states;
- separation between service-authored audit evidence and local UI event buffer;
- browser-level fixture, unavailable, and audit/security legibility states with
  Playwright or Cypress;
- screenshot or trace artifact generation for at least one browser reliability
  path; and
- non-persistence of secret-like fields in browser storage.

## Suggested Engineering Sequence

1. Inspect service audit store APIs and current security status RPC.
2. Choose the narrow service response shape and add protobuf/codegen tests.
3. Implement the service-side read path over existing audit/security state.
4. Add public service tests before wiring the gateway.
5. Add `/api/security/audit/status` to the gateway with redacted errors.
6. Add frontend types, API fetch, fixtures, store state, and UI rendering.
7. Add the Playwright or Cypress reliability baseline over fixture and
   unavailable gateway states; add secured-local E2E only if it stays small and
   reliable.
8. Update docs and run the expected checks.
9. Review all response/log fields for secret, filesystem path, policy, and
   graph-value leakage before handoff.

## Memory Updates After Merge

After the PR merges, update GRM project memory:

- mark WorkSlice 602 completed only after code and public tests prove the read
  path;
- link any new implementation facts to RoadmapItems 13 and 248 if not already
  linked;
- keep Risk 587 active unless an accepted admin-boundary design exists;
- update or create a `SecurityControl` only for implemented and tested
  read-only audit observability;
- do not mark live administration, permission mutation, hosted identity,
  external audit forwarding, attestation, receipts, state commitments, hosted
  durability, or high-assurance audit as implemented.
