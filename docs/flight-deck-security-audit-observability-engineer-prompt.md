# GRM Flight-Deck Security/Audit Observability Engineer Prompt

## Goal

Implement the next flight-deck slice: read-only security and audit
observability for the secured-local Admin no.1 journey.

This slice should make the first-time secured-local flow understandable from
the flight-deck before any administration UI exists. A user should be able to
see:

- who they are;
- how they are connected; and
- what security/audit evidence was recently produced.

The guiding product phrase is: **observability before administration**.

## Why This Matters

ADR 0011 created a passwordless, file/helper-driven secured-local bootstrap
path for Admin no.1. The merged security-status slice made identity and
connection profile metadata visible in flight-deck without exposing secrets or
introducing live policy mutation.

The next gap is legibility. A user who completes Admin no.1 bootstrap should
not have to infer from terminal output and generated files that GRM accepted
their certificate, resolved their principal, applied the expected profile, and
recorded bounded audit evidence. The flight-deck should show that safely.

This is still not an admin plane. It is a read-only confidence layer over
implemented service/security truth.

## Starting Point

- Work in `/home/laurie/source/grm-rs`.
- Start from the merged `main` flight-deck and security-status work.
- Read the current security and flight-deck context:
  - `docs/adr/0008-bounded-authoritative-security-audit.md`
  - `docs/adr/0009-grm-native-audit-log-store.md`
  - `docs/adr/0011-passwordless-secured-local-bootstrap.md`
  - `docs/security/security-audit.md`
  - `docs/security/security-design.md`
  - `grm-flight-deck/README.md`
  - `grm-flight-deck-gateway/src/main.rs`
  - `grm-service-api/src/lib.rs`
- Treat WorkSlice 584 as completed implementation truth: flight-deck already
  has read-only `/api/security/status`.
- Treat Risk 587 as active: admin-plane mutation must not sneak into this
  work.

## Scope

### 1. Read-Only Audit/Status Service Surface

Add the smallest read-only service/gateway surface needed for flight-deck to
display audit/security observability.

Prefer a narrow endpoint such as:

```text
GET /api/security/audit/status
```

or, if the service boundary already supports safe event access cleanly:

```text
GET /api/security/audit/events?limit=N
```

The surface should expose only bounded, redacted, UI-safe data. Useful fields
may include:

- audit mode or profile;
- audit sink health;
- whether mandatory secured audit is currently available;
- bounded recent event count;
- bounded retained event count if already available;
- last append/recovery status if already modeled safely;
- recent safe event summaries, only if the service already has a suitable
  read path or one can be added narrowly.

If recent events are included, keep each event summary small and redacted. Good
summary fields include:

- event time if already present and safe;
- request or correlation id if bounded and non-sensitive;
- operation family/action;
- authenticated principal issuer/subject if already stored safely;
- authentication method;
- policy version;
- decision or bounded reason code;
- outcome stage such as authentication, authorization, admission, runtime, or
  durable mutation;
- workspace reference only if already opaque and safe.

### 2. Flight-Deck UI

Extend the existing flight-deck security context area so Admin no.1 can see:

- current security profile;
- current identity/principal;
- authentication method;
- policy version;
- audit availability/health;
- recent bounded audit activity or an explicit "not available in this profile"
  state.

Keep the UI compact and operational. This is a workbench status surface, not a
marketing page and not an admin wizard.

Fixture mode should include realistic safe fixture status/audit data so the UI
remains demonstrable without a running secured service.

### 3. Error Handling And Redaction

Gateway and browser errors must remain stable and non-sensitive.

Do not expose:

- raw certificates;
- private keys;
- client-certificate paths;
- private-key paths;
- certificate fingerprints;
- rendered permission tables;
- policy internals;
- raw graph values;
- unbounded audit payloads;
- local filesystem paths;
- bearer tokens, passwords, or future credential material.

Normal gateway logs must also avoid cert/key paths and credential material.

### 4. Documentation

Update flight-deck and secured-local documentation just enough to explain the
new read-only observability:

- what a successful Admin no.1 status looks like;
- how anonymous_local and docker_local_insecure should appear;
- what audit health/status means in local secured profiles;
- what remains file/helper-driven;
- what is still not implemented.

## Non-Goals

- No live service-side user, principal, access-level, role, or permission
  mutation APIs.
- No admin plane.
- No policy editor.
- No permission table viewer unless a separate accepted design explicitly
  approves it.
- No browser private-key, client-certificate, password, bearer-token, or raw
  credential handling.
- No production or hosted identity lifecycle.
- No external/high-assurance audit forwarding.
- No attestation, signed receipts, state commitments, or client-verifiable
  continuity.
- No new public query language.
- No 3D/spatial work.

## Constraints

- Preserve the typed service direction. Do not introduce textual adapter-only
  semantics as the service contract.
- Keep security claims limited to implemented local profiles and public tests.
- Keep anonymous_local and docker_local_insecure visibly weaker than secured.
- Keep secured-local additive; it must not become the default startup profile.
- Maintain the distinction between transport peer, authenticated principal,
  asserted actor, administrator, policy decision, audit evidence, and runtime
  outcome.
- Fail closed for secured-profile audit unavailability where existing security
  rules require it.
- Prefer existing service/audit types and helpers over creating a parallel
  audit model.

## Acceptance Criteria

- Flight-deck shows identity, connection/security profile, auth method, policy
  version, and audit availability/status together.
- Admin no.1 secured-local fixture or real local run is legible from the UI.
- Anonymous local has an explicit readable status and does not masquerade as
  secured.
- Docker-local insecure has an explicit readable status and does not
  masquerade as secured.
- Browser responses do not include cert/key paths, fingerprints, raw certs,
  permission tables, or policy internals.
- Normal gateway logs do not include cert/key paths or credential material.
- The implementation remains read-only.
- Documentation describes the implemented truth and preserves non-goals.

## Expected Checks

- `cargo test -p grm-service-api` for any service/API changes.
- Focused public-boundary service tests for safe audit/status data and
  redaction.
- `cargo test -p grm-flight-deck-gateway`.
- Gateway tests proving public and normal-log errors do not expose local
  cert/key paths.
- `npm run build` in `grm-flight-deck`.
- Frontend tests for rendering secured, anonymous_local, docker_local_insecure,
  fixture, unavailable, and error states if the test harness exists or is added
  in this slice.

## Skills To Use

- `grm-product-manager`
- `grm-security-engineer`
- `grm-project-memory`
- `grm-project-constraints`

## Memory Updates After Merge

After the PR merges, update GRM project memory:

- mark the new security/audit observability work slice completed;
- link it to the flight-deck roadmap and security-core roadmap;
- keep Risk 587 active unless an accepted admin-boundary design exists;
- record any new read-only security control as implemented only if tests prove
  it;
- do not mark live administration, permission mutation, hosted identity,
  external audit, attestation, receipts, or state commitments as implemented.
