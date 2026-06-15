---
name: grm-security-engineer
description: Use when reviewing, designing, or implementing security-facing code in grm-rs, including authentication, identity, authorization, workspace isolation, request limits, audit, TLS/mTLS, secrets, encryption at rest, key management, durable outcomes, signed receipts, state commitments, supply-chain controls, or security product claims. Inspect the live GRM security graph and implementation before judging whether a change advances the accepted security direction.
---

# GRM Security Engineer

Evaluate security-facing GRM work against the live threat, requirement, control,
boundary, identity, question, and decision graph. Use code and tests as the
source of truth for implementation status.

## Operating Rules

- Use `grm-project-constraints` alongside this skill when available.
- Treat the connected project-memory database as protected SOML. Never run
  destructive or broad cleanup operations against it without explicit user
  approval.
- Do not infer that a proposed graph control is implemented.
- Do not infer application identity or authorization from TLS/mTLS alone.
- Do not accept client-supplied actor, permission, workspace, operation
  classification, policy decision, or audit outcome as trusted.
- Keep security claims limited to deployment profiles and public tests that
  prove them.

## Startup

1. Call `grm_schema_list`.
2. If Neo4j mode is active, read `grm://backend/status`.
3. Inspect relevant nodes:
   - `TrustBoundary`
   - `IdentityKind`
   - `Threat`
   - `SecurityRequirement`
   - `SecurityControl`
   - `SecurityOpenQuestion`
   - `SecurityDecision`
4. Follow relevant relationships:
   - `THREAT_TARGETS_BOUNDARY`
   - `IDENTITY_ESTABLISHED_AT`
   - `CONTROL_ESTABLISHES_IDENTITY`
   - `CONTROL_APPLIES_TO_BOUNDARY`
   - `CONTROL_MITIGATES_THREAT`
   - `REQUIREMENT_ADDRESSES_THREAT`
   - `REQUIREMENT_REQUIRES_CONTROL`
   - `REQUIREMENT_APPLIES_TO_BOUNDARY`
   - `QUESTION_CONCERNS_REQUIREMENT`
   - `DECISION_RESOLVES_SECURITY_QUESTION`
   - `DECISION_SATISFIES_SECURITY_REQUIREMENT`
   - `DECISION_SELECTS_SECURITY_CONTROL`
5. Read only the security documents relevant to the task:
   - `docs/security/threat-model.md`
   - `docs/security/security-design.md`
   - `docs/security/security-memory-schema.md`
6. Inspect the affected implementation and public test surfaces.

If the security graph is unavailable, say so. Use the Markdown documents as a
degraded fallback, but do not present graph-backed direction as confirmed.

## Establish Scope

Before reviewing or editing, identify:

- deployment profile: embedded, local development service, secured service, or
  future hosted service;
- affected trust boundaries;
- identity kinds entering and leaving the code path;
- protected assets and active threats;
- applicable accepted security decisions;
- requirements the change claims to satisfy;
- controls being introduced, changed, bypassed, or relied upon; and
- required verification evidence.

If an open security question determines the proposed API or guarantee, classify
the work as blocked by design rather than selecting a contract accidentally in
code.

## Review The Enforcement Path

For service-facing operations, verify the intended order:

1. Validate transport and collect transport-peer evidence.
2. Decode and normalize typed input.
3. Authenticate an application principal.
4. Resolve workspace, action, operation family, and resource server-side.
5. Authorize with explicit policy and fail closed.
6. Apply finite admission and resource limits.
7. Record a bounded, redacted attempt where required.
8. Execute through the canonical typed runtime path.
9. Establish the durable mutation outcome.
10. Record runtime, commit, and delivery outcomes distinctly.
11. Return only evidence supported by implemented semantics.

Reject alternate adapter, alias, batch, admin, or direct-RPC paths that bypass
the canonical `ExecuteWorkspace` enforcement route.

## Security Review Lenses

### Identity

- Keep transport peer, authenticated principal, asserted actor, delegated
  actor, service identity, and administrator distinct.
- Treat actor metadata as an assertion until authenticated delegation binds it.
- Ensure authentication providers return identity, not permissions.
- Check credential parsing, expiry, issuer, audience, mapping, revocation, and
  secret redaction where applicable.

### Authorization And Isolation

- Derive actions and resources from typed operations on the server.
- Default deny in secured profiles and fail closed on policy errors.
- Authorize every contained batch operation and stricter administrative action.
- Bind workspace handles to stable workspace identity and policy.
- Look for cross-workspace existence leaks as well as successful access.

### Validation And Runtime Safety

- Preserve schema validation, transaction safety, delete controls, backend
  capability checks, and durability invariants after authorization.
- Do not introduce security semantics through adapter-only parsing or textual
  query strings.
- Do not report durable success before commit outcome is established.

### Confidentiality And Secrets

- Check response, error, log, audit, trace, metric, profile, and explain paths
  for credentials, graph values, private paths, policy details, and identity
  leakage.
- Distinguish TLS from application authentication.
- Distinguish host disk encryption from GRM-managed envelope encryption.
- Keep TLS, token-verification, encryption, receipt-signing, and attestation
  keys purpose-separated.

### Availability

- Bound request and response bytes, batch size, traversal depth, result count,
  profile cost, deadlines, concurrency, handles, rate, audit queues, and storage
  pressure where the deployment claim requires them.
- Verify cancellation and retry behavior cannot obscure committed mutations.
- Reject unbounded process-local audit or security history.

### Audit And Evidence

- Distinguish authenticated principal, actor assertion, policy decision,
  runtime outcome, durable outcome, and response-delivery outcome.
- Treat audit, provenance, signed receipts, state commitments, and attestation
  as different evidence classes.
- Do not claim a signature proves truth or that a hash chain proves honest
  original input.
- Keep client-verifiable receipts and commitments deferred until stable
  identity, policy, audit, and commit semantics exist.

### Supply Chain

- Review security-sensitive dependency, generated-contract, workflow, release,
  provenance, and credential-scope changes.
- Prefer minimal trusted-publishing permissions and artifact-content checks.
- Keep release signing separate from runtime receipt signing.

## Evidence And Tests

Require tests at the boundary owning the behavior:

- TLS and mTLS trust: shared gRPC service tests.
- Authentication, authorization, isolation, limits, and audit: public service
  integration tests.
- Runtime invariants: runtime tests.
- Encryption, key rotation, tamper detection, reopen, backup, and recovery:
  storage and durability tests.
- Receipt binding, signatures, continuity, rollback, and fork handling:
  service/client integration tests.
- Adapter-specific credential configuration and errors: adapter tests.

For each allow path, look for relevant negative cases:

- missing or malformed credential;
- wrong principal;
- asserted actor impersonation;
- wrong workspace;
- unauthorized contained batch operation;
- policy load or evaluation failure;
- malformed or ambiguous request;
- limit exceeded;
- replay or retry ambiguity;
- secret or existence leakage; and
- interrupted mutation with an established commit outcome.

Do not mark a `SecurityRequirement` verified solely because a unit test, design
document, or happy-path test exists.

## Review Output

For code review, lead with findings ordered by severity. Each finding should
include:

- file and line;
- failure or exploit scenario;
- affected boundary and threat;
- violated requirement or accepted decision;
- why current controls are insufficient; and
- the smallest corrective action or missing test.

Then report:

- **Direction assessment**: aligned, partial, blocked, risk, or later.
- **Implemented truth**: controls actually present and tested.
- **Open decisions**: graph questions that must be resolved.
- **Residual risk**: guarantees still unsupported after the change.

If there are no findings, say so and identify remaining test or deployment
limits.

## Implementation Guidance

When implementing security work:

- choose the smallest slice satisfying one or two explicit requirements;
- reuse the typed workspace/runtime path;
- introduce trusted internal context rather than client-authored effective
  security context;
- use stable typed error categories without sensitive detail;
- preserve current local mode only through an explicit weaker profile;
- add negative public-boundary tests with the implementation; and
- update graph control/requirement status only after code and verification make
  the new status true.

Do not resolve unrelated future questions, introduce a general policy language,
or implement attestation/commitment machinery under a narrower identity or
authorization slice.

## Memory Maintenance

After an accepted security decision or implemented control:

- update or create the applicable `SecurityDecision`;
- resolve connected `SecurityOpenQuestion` nodes;
- update `SecurityControl` status accurately;
- update `SecurityRequirement` status only when its verification evidence is
  satisfied;
- add missing threat, boundary, or identity links exposed by the work; and
- keep security docs synchronized with implementation truth.

Use `grm_schema_checkpoint` after deliberate security schema-memory changes
when an explicit checkpoint is useful. It compacts schema memory only and does
not modify Neo4j graph data.
