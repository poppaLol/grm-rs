# ADR 0008: Require Bounded Redacted Audit At Security And Effect Boundaries

Status: Accepted

Date: 2026-06-22

## Context

GRM's service security design distinguishes transport validation,
authentication, server-side scope resolution, authorization, admission,
runtime execution, durable mutation outcome, and response delivery. Phase 1
implements part of that pipeline, but it has no security audit event model or
sink. Current lifecycle output is ordinary process logging and does not provide
trusted, bounded, correlated security evidence.

Audit is security-sensitive because a client must not choose the identity,
operation classification, policy decision, or execution outcome recorded by
the service. Audit is also availability-sensitive: making an authoritative
sink mandatory means backpressure, storage failure, or sink unavailability can
prevent secured operations. That trade-off must be selected explicitly by
deployment posture rather than inherited accidentally by local development.

The first implementation needs a bounded local sink, but the event and sink
contracts must not assume that audit will always remain process-local or
file-local. Future deployments may use external audit, monitoring, or security
analysis systems while preserving event meaning and truthful outcomes.

## Decision

GRM will define a versioned, principal-centric security audit event and a
bounded authoritative sink contract. The service authors audit records from
trusted internal identity, scope, decision, and outcome state. Clients and
adapters cannot submit effective audit records or outcomes.

The event contract distinguishes:

1. request attempt;
2. authentication result;
3. authorization decision;
4. admission or limit result;
5. runtime outcome;
6. durable mutation outcome; and
7. response handoff, cancellation, or unknown delivery outcome.

These stages may be represented by correlated records or a bounded append-only
record sequence. Their meaning and ordering within one request are stable and
must not be collapsed into a single ambiguous success or failure field.

## Deployment Audit Postures

Audit strictness is selected explicitly with deployment security posture.

### `anonymous-local`

Best-effort audit or ordinary operational logging is acceptable. Sink failure
does not need to deny local development operations. This posture provides no
security-audit completeness, retention, or availability guarantee and must not
be described as secured audit.

### `secured-profile`

A bounded authoritative audit sink is mandatory. Required pre-effect records
must be accepted within finite capacity and time before an effect executes. If
the authoritative sink cannot accept them, the service returns a stable
unavailable outcome and does not execute the operation.

This deliberately trades availability for audit completeness at the security
boundary. Queue saturation, append timeout, retention failure, degraded
storage, and authoritative sink unavailability can therefore reduce secured
service availability.

If audit failure occurs after a mutation has committed, the service must not
rewrite or obscure the established commit outcome. It reports that outcome
truthfully, marks audit health degraded, emits any available bounded emergency
signal, and rejects subsequent secured effects until the authoritative sink
recovers. Audit failure cannot roll back an already committed mutation.

### `high-assurance/regulated`

This is future architectural direction, not an implemented GRM deployment
profile or current compliance claim. Such a posture may require an external
authoritative sink, stronger access and retention controls, monitored delivery,
separation of duties, and independently tested sink availability and recovery.

If an external system is configured as authoritative, its acceptance latency,
backpressure, and failure directly affect service availability. Those
operational guarantees require a separate deployment contract and public
evidence.

## Principal-Centric Identity

Audit identifies the canonical authenticated principal independently of
credential mechanism. The authentication provider and method are evidence
attributes, not principal categories. Workload, service, and user principals
use the same canonical audit fields.

The record separately labels:

- transport-peer evidence;
- authenticated principal;
- asserted actor;
- delegated actor and delegation reference;
- service identity; and
- administrative authority when applicable.

An asserted actor must never replace the authenticated principal in the audit
record. Future delegated or end-user pass-through models require explicit
contracts defining how service and user identities are represented,
authorized, and audited. Audit must not silently attribute a service-principal
operation to an asserted end user.

## Event Contract

Each event contains only bounded, typed fields appropriate to its stage:

- event schema version, event ID, request correlation ID, and service sequence;
- timestamp and service identity reference;
- bounded transport-peer evidence reference;
- canonical principal issuer, subject, authentication provider, and method
  when authentication succeeds;
- separately labelled bounded asserted or delegated actor references;
- stable workspace identity when resolved;
- server-derived actions and protected resource classifications;
- policy version, decision, and bounded reason code;
- admission or limit classification;
- runtime outcome;
- durable outcome; and
- response handoff, cancellation, or unknown delivery state when observable.

Authentication failure records do not invent a principal. A read or
non-mutating operation uses a `not_applicable` durability outcome. Mutation
durability is reported as `committed`, `not_committed`, or `unknown` only when
the implementation evidence supports that distinction.

The service may record that it handed a response to its transport, observed
cancellation, or does not know the delivery result. It must never claim that a
client received or processed a response when that cannot be observed.

## Redaction And Bounds

Audit excludes by construction:

- credentials, bearer tokens, private keys, and raw certificate bodies;
- graph property values and unbounded request or response bodies;
- server-local filesystem paths;
- policy documents or mapping-table contents;
- unbounded error text, actor metadata, model lists, or trace attributes; and
- future receipt, attestation, or state-proof material unless separately
  accepted into the audit schema.

Strings, identifiers, classification counts, event sizes, queue capacity,
append latency, storage bytes, and retention age have finite configured maxima.
If trusted operation classification cannot fit the audit bound, the secured
profile rejects the request before execution with a bounded overflow record.
It does not truncate classification in a way that hides a contained or
underlying operation.

Retention is finite by both maximum bytes and maximum age. Expiry and rotation
are deterministic and must not create unbounded process-local history. Access
to retained security audit is a separately authorized administrative concern.

## Authoritative Sink And Forwarding

The versioned event model is independent of storage or forwarding architecture.
The first implementation may use a bounded local authoritative sink. Future
implementations may use local durable storage, an external authoritative sink,
or bounded forwarding to audit, monitoring, or security-analysis systems
without changing event meaning.

Mandatory audit acceptance occurs when the configured authoritative sink
accepts the record according to its documented durability contract. Optional
downstream forwarding occurs after authoritative acceptance and may retry using
stable event IDs. Optional forwarding failure does not retroactively invalidate
an authoritative local acceptance.

Ordering guarantees are scoped to one request and the service sequence emitted
by one service instance. GRM does not claim universal total ordering across
distributed sinks, services, or forwarding retries. Consumers must tolerate
duplicate delivery where forwarding semantics are at-least-once.

The sink contract must expose bounded capacity, acceptance timeout, health,
retention behavior, recovery behavior, and whether acceptance is in-memory,
persisted locally, or acknowledged externally. A process-local unbounded vector
is never an authoritative secured-profile sink.

## Failure And Backpressure Contract

For `secured-profile`:

- mandatory pre-effect events cannot be silently dropped;
- bounded queue or append backpressure expires after a finite deadline;
- failure to accept an attempt or allow decision denies execution;
- authentication, denial, policy-error, and limit outcomes remain mandatory
  security events even though they do not execute runtime effects;
- capacity for required post-effect outcome records should be reserved before
  executing a mutation;
- post-commit audit failure preserves the truthful commit result and degrades
  service audit health; and
- service recovery does not resume secured effects until authoritative sink
  health and pending outcome handling meet the configured contract.

This decision does not promise atomic transactionality between workspace state
and audit storage. The implementation must expose any residual interval in
which a process or storage failure can commit workspace state without recording
the final audit outcome.

## Required Public-Boundary Proof

Public gRPC service integration tests must prove:

- correlated attempt, authentication, authorization, admission, runtime,
  durability, and delivery-state records for the stages reached;
- allowed, denied, unauthenticated, policy-error, over-limit, malformed, and
  runtime-failure paths;
- consistent principal fields for workload, service, and user test principals
  regardless of authentication provider;
- asserted actors remain separate and cannot replace the principal;
- bounded field, operation-classification, queue, event-size, byte-retention,
  and age-retention behavior;
- redaction of credentials, certificates, keys, graph values, request bodies,
  private paths, and unbounded errors;
- per-request stage ordering and stable correlation IDs;
- pre-effect authoritative sink failure and backpressure deny execution;
- post-commit failure reports the commit truthfully, degrades audit health, and
  blocks subsequent secured effects;
- anonymous-local remains explicitly best-effort and makes no completeness
  claim;
- reopen and recovery behavior for the first authoritative local sink; and
- optional forwarding retries preserve event IDs and do not alter event
  meaning.

Private sink contract tests may supplement public proof for rotation, recovery,
queue scheduling, duplicate forwarding, and deterministic retention.

## Non-Goals

- No claim of client receipt or processing.
- No unbounded process-local audit history.
- No tamper-evident log, signed receipt, state commitment, non-repudiation, or
  attestation.
- No universal ordering across distributed services or sinks.
- No general telemetry, monitoring, or incident-management platform.
- No hosted, high-assurance, regulated, or compliance claim.
- No broad request-limit programme beyond bounds required to make audit safe.
- No encryption-at-rest design or key-management contract.
- No delegation, end-user pass-through, tenancy, or ownership semantics.
- No assumption that local files are the permanent audit architecture.

## Consequences

Positive consequences:

- Audit identity and outcomes remain stable as authentication providers evolve.
- Security-relevant records are service-authored, bounded, redacted,
  correlated, and explicit about what the service can observe.
- The authoritative-sink boundary makes audit completeness and forwarding
  semantics testable.
- A local first sink does not prevent external audit or security-analysis
  integration.
- Deployment posture makes strict audit an explicit security choice.

Tradeoffs:

- Secured-profile availability depends on authoritative audit health and
  capacity.
- Post-commit sink failure cannot be made equivalent to a rolled-back mutation.
- Bounded retention intentionally expires older evidence.
- External authoritative sinks add latency, network failure, operational
  dependency, and duplicate-delivery concerns.
- Stronger high-assurance or regulated requirements need separate architecture,
  operations, and evidence.

## Relationship To Existing Decisions

This decision builds on ADR 0006's canonical principal boundary and ADR 0007's
server-derived authorization scope. It preserves the accepted distinctions
between transport peer, authenticated principal, actor assertion, policy
decision, runtime safety, durable outcome, state commitments, and attestation.

It resolves the first mandatory audit event and sink contract. It does not make
the audit control implemented, durable across every failure, tamper-evident, or
suitable for hosted, high-assurance, regulated, or compliance claims.
