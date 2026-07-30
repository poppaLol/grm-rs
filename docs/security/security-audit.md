# GRM Security Audit Concepts

This document explains the first local bounded authoritative security-audit
slice for the GRM local gRPC workspace service. It is the operator and reviewer
companion to [ADR 0008](../adr/0008-bounded-authoritative-security-audit.md),
which records the durable design decision.

The current implementation is intentionally narrow. It gives the secured local
service a service-authored, durable local, bounded, redacted audit trail for security-relevant
workspace requests. It does not provide external audit forwarding,
tamper-evident storage, arbitrary-failure recovery, hosted or public MCP identity,
signed receipts, state commitments, attestation, or high-assurance compliance
claims.

Future flight-deck work should make this audit stream inspectable alongside the
query flight-deck, operational log, backend status, and security context. Until
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

The local service binary uses a service-owned store under
`<service-root>/audit/`, physically separate from workspace checkpoints,
workspace append logs, user graph data, schema memory, and Neo4j project
memory. Its authoritative representation is an ordered list of typed immutable
`SecurityAuditEvent` records. Stable identifiers and service sequence carry
correlation; graph relationships are optional rebuildable flight-deck projections.

Acceptance writes one complete versioned record and newline, synchronizes the
file, and synchronizes the audit directory when the log is first created.
Initialization synchronizes generation metadata and newly created directory
entries through the service root. A final unterminated record is ignored as a
torn tail; a malformed complete record, unsupported version, conflicting
generation, or exhausted counter makes mandatory audit unavailable.

The generation ID persists across normal restart. Request IDs reserve bounded
ranges of 64; event IDs and service sequences reserve paired ranges of 64 in a
single synchronized metadata replacement. Accepted high-water marks advance
only after the event is durable. A crash can lose at most one reserved range
per counter at a reservation boundary, but reopening starts above the durable
range ceiling and cannot duplicate identity within one generation.

Best-effort profiles do not turn audit identity-allocation failure into an
application failure. If durable request or event identity allocation fails,
that request switches permanently to process-local-only correlation and makes
no further durable audit calls. Fallback identifiers are never written to the
durable store. Mandatory secured audit continues to fail closed.

Retention is finite by event count, serialized retained bytes, age, and maximum
record size. When expiry removes events, the store atomically replaces the
physical log with a synchronized retained suffix and syncs the directory.
Events are immutable while retained; ordinary writes append, while physical
compaction may replace the file.

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
operator-visible flight-deck state, and any configured process-stop or failover
policy.

The next secured request may perform a bounded synchronous recovery probe before
remaining blocked. Recovery validates durable metadata and generation, every
complete bounded event, ordering, reservation high-water marks, retention, and
directory durability. Torn tails and retention changes use the normal atomic
compaction protocol. Without compaction, the active log is explicitly synced;
accepted high-water metadata is then reconciled and durably replaced before the
directory is synced. Only a completely successful probe replaces memory,
clears sink and service degradation, and permits the request to continue.
Malformed complete records, generation conflicts, metadata inconsistency, or
filesystem failure remain fail-closed. Anonymous-local and Docker-local do not
require this recovery before application effects because their audit posture is
explicitly best effort.

Audit timestamps come from the service wall clock. They are useful evidence but
are neither monotonic nor tamper-proof. If NTP correction, VM restoration, or an
operator clock change makes a persisted event appear future-dated, recovery
retains the original timestamp and reports a bounded anomaly count instead of
making the store unavailable. Future-dated events remain subject to count and
byte retention and are not expired by age until wall time passes their timestamp
plus the configured maximum age.

## Flight-Deck Direction

The future flight-deck should make audit understandable without turning it into a
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
- cross-links to the operational log and query/explain flight-deck for the same
  workspace or request.

The flight-deck should not expose raw credentials, raw certificates, graph property
values, private paths, policy table contents, or unbounded request and response
bodies.

## Current Non-Claims

The first local bounded authoritative audit slice does not claim:

- external authoritative audit;
- durable audit recovery beyond the tested single-process honest-local-filesystem scope;
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
