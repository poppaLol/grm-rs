# GRM Security Audit Concepts

This document explains the first local bounded authoritative security-audit
slice for the GRM local gRPC workspace service. It is the operator and reviewer
companion to [ADR 0008](../adr/0008-bounded-authoritative-security-audit.md),
which records the durable design decision.

The current implementation is intentionally narrow. It gives the secured local
service a service-authored, bounded, redacted audit trail for security-relevant
workspace requests. It does not provide external audit forwarding,
tamper-evident storage, recovery persistence, hosted or public MCP identity,
signed receipts, state commitments, attestation, or high-assurance compliance
claims.

Future cockpit work should make this audit stream inspectable alongside the
query cockpit, operational log, backend status, and security context. Until
that UI exists, the audit model should be understood through the event fields,
modes, and failure behavior described here.

## What The Audit Is For

Security audit records answer questions such as:

- who was authenticated as the application principal;
- whether an actor was merely asserted by metadata;
- which workspace and operations the service derived from the request;
- which policy version and decision applied;
- whether admission limits or classification bounds rejected the request;
- whether runtime execution succeeded or failed;
- whether a durable mutation outcome is known; and
- whether the service handed the response to the transport.

The audit is not a debug log, tracing stream, metrics system, or full incident
management platform. It deliberately excludes graph property values, raw
credentials, raw certificates, private keys, local filesystem paths, unbounded
request bodies, unbounded error text, and policy table contents.

## Audit Modes

Audit mode follows the service security profile.

### Mandatory Audit

The secured profile uses mandatory audit. Required pre-effect records must be
accepted by the configured authoritative sink before a secured effect runs.

If mandatory audit cannot accept a required pre-effect record, the service
returns an unavailable-style security error and does not execute the requested
effect. This is intentional: a secured service trades availability for audit
completeness at the security boundary.

Mandatory audit is the only current mode that supports a security-audit
guarantee. It is still scoped to the first local bounded authoritative audit
slice, not to hosted, external, durable-across-all-failures, or tamper-evident
audit.

### Best-Effort Audit

The anonymous-local profile uses best-effort audit. This preserves local
development behavior: a local scratch workflow should not fail merely because a
process-local audit sink is unavailable.

Best-effort audit must be described as local development observability only. It
is lossy by design and makes no security-audit completeness, retention, or
availability claim. A deployment that needs audit for security must use the
secured profile and mandatory audit behavior.

## Event Shape

Every security audit event is versioned and service-authored. Clients and
adapters do not submit effective audit outcomes.

Events include:

- schema version;
- event ID, request ID, and service sequence;
- timestamp and service identity;
- audit mode;
- stage;
- transport peer evidence;
- authenticated principal, when authentication succeeds;
- asserted actor, when supplied;
- workspace, when resolved;
- server-derived operations and resource classifications;
- policy version;
- decision and reason;
- runtime outcome;
- durability outcome; and
- delivery outcome.

All identity, workspace, model, and operation-classification fields are
bounded. If trusted classification cannot fit the configured audit bound, the
secured profile rejects the request before execution and records a bounded
classification-overflow event where possible.

## Stages

Audit records are correlated by request ID and ordered by service sequence.
Not every request reaches every stage.

| Stage | Meaning |
| --- | --- |
| `Attempt` | The service received a request and started audit handling. |
| `Authentication` | The service authenticated, rejected, or could not establish the application principal. |
| `Authorization` | The service evaluated server-derived operations and resources against policy. |
| `Admission` | The service applied request-shape, scope, classification, and limit checks before runtime execution. |
| `Runtime` | The canonical workspace/runtime path ran, succeeded, or failed. |
| `Durability` | The service recorded the mutation durability evidence it can truthfully support. |
| `Delivery` | The service recorded response handoff or an audit-sink delivery problem it could observe. |

The important product rule is that these are distinct evidence classes. A
successful authorization decision is not a runtime result. A runtime success is
not automatically a durable commit. A transport handoff is not proof that a
client processed the response.

## Identity Model

Audit is principal-centric. The authenticated principal is the canonical
application identity established by the configured authentication provider.

The asserted actor is separate. For example, an MCP adapter may assert a caller
or tool context, but that assertion does not replace the authenticated backend
principal and does not grant authority by itself.

The first slice does not implement delegated actor semantics, end-user
pass-through identity, hosted tenancy, browser auth, OAuth, or public MCP
identity. Those need separate design and tests before they can become audit
claims.

## Runtime, Durability, And Delivery

The audit records the outcome the service can support with evidence.

Runtime outcome describes whether the service's canonical workspace/runtime
path succeeded or failed.

Durability outcome is narrower:

- read-only and in-memory lifecycle operations use `NotApplicable`;
- local-autocommit workspace create can report `Committed`;
- failed runtime paths report `NotCommitted` when no durable mutation was
  established; and
- `Unknown` remains available for paths where the service cannot truthfully
  distinguish the durability result.

Delivery outcome is also limited. The service can record that it handed a
response to the transport. It cannot claim that a client received, processed,
or acted on that response.

## Sink Behavior

The first implementation uses a bounded local audit sink. Its retention is
finite by event count, retained bytes, and age. This avoids an unbounded
process-local security history.

Sink append can fail because the sink is unavailable, under backpressure, or
because the event is too large. In mandatory audit, failures before an effect
deny the effect. In best-effort audit, append failure does not fail the local
development operation.

## Degraded Audit

If a mandatory audit append fails after a mutation has already committed, the
service must not hide or rewrite the established mutation outcome. It records
what it can, marks audit health degraded, and blocks subsequent secured effects
until the authoritative sink reports healthy again.

Product-wise, degraded mandatory audit should be treated as a secured-service
health problem. The current slice blocks later secured effects. Future
operability work should expose this state through health/readiness reporting,
operator-visible cockpit state, and any configured process-stop or failover
policy.

## Cockpit Direction

The future cockpit should make audit understandable without turning it into a
general SIEM or hosted control plane.

Useful audit views include:

- per-request timeline grouped by request ID;
- stage sequence with decision, reason, runtime, durability, and delivery
  outcomes;
- principal and asserted-actor columns shown separately;
- policy version and server-derived operation/resource classifications;
- sink mode, retention limits, and health/degraded state;
- redaction and overflow indicators;
- filters for denied, unavailable, runtime-failed, committed, and degraded
  events; and
- cross-links to the operational log and query/explain cockpit for the same
  workspace or request.

The cockpit should not expose raw credentials, raw certificates, graph property
values, private paths, policy table contents, or unbounded request and response
bodies.

## Current Non-Claims

The first local bounded authoritative audit slice does not claim:

- external authoritative audit;
- durable audit recovery across every process or storage failure;
- tamper-evident audit storage;
- signed receipts;
- state commitments;
- non-repudiation;
- attestation;
- hosted or public MCP identity;
- hosted tenant isolation;
- production PKI lifecycle;
- broad request-limit coverage beyond the current tested controls; or
- high-assurance, regulated, or compliance suitability.

Those are future work and should be added only with explicit design, tests, and
updated product language.
