# ADR 0009: Use A GRM-Native Service-Owned Audit Log Store

Status: Accepted

Date: 2026-07-07

## Context

[ADR 0008](0008-bounded-authoritative-security-audit.md) accepts a
versioned, principal-centric audit event and bounded authoritative sink
contract. The first implemented slice proves the event model and mandatory
secured-profile behavior with a bounded process-local sink. That is enough to
validate service-authored audit semantics, but it is not enough for operator
inspection, restart-surviving evidence, or a future cockpit audit-log tab.

GRM already has local append-log mechanics for workspace durability: append
newline-delimited JSON records, flush and sync each record, recover complete
newline-terminated records, ignore a torn final record, and abort on malformed
complete records. Those mechanics are not a full WAL with epochs, checksums,
cross-process fencing, or point-in-time recovery, but they provide a small
local persistence pattern that fits GRM's current single-process local service
target.

The audit persistence decision must preserve security boundaries:

- audit is service-owned evidence, not user graph data;
- authentication failures and denied requests may happen before a workspace is
  resolved;
- audit must not depend on the workspace mutation log it is auditing;
- audit events must stay bounded, redacted, service-authored, and
  deployment-scoped; and
- future cockpit inspection should read audit evidence without turning the
  source of truth into a general SQL event store.

Off-the-shelf storage options such as SQLite would make query and UI work
easier, but they would also make the first durable audit representation a
general-purpose SQL database rather than a GRM-owned operational-memory
primitive. That is a poor fit for the product direction unless later evidence
shows a table/index projection is required.

## Decision

GRM will use a service-owned GRM-native audit log store as the future durable
local persistence direction for security audit events.

The audit log store is a distinct service storage component. It is not the
workspace WAL, not user graph state, not Neo4j project memory, and not an
external monitoring system. It stores a bounded append-only list of
`SecurityAuditEvent` records under service-owned storage, with recovery and
retention semantics designed for local secured-service audit evidence.

The first durable implementation should extract or reuse a small generic
append-log primitive from the existing durability code rather than introducing
SQLite or another embedded database. The primitive should support a simple
event-list store such as:

```rust
struct AuditEventStore {
    log: JsonAppendLog<SecurityAuditEvent>,
    retained: VecDeque<SecurityAuditEvent>,
    limits: AuditRetentionLimits,
}
```

The log may use newline-delimited JSON initially if that keeps the slice small
and consistent with existing durable operation logs. A later framed format with
lengths, checksums, segment IDs, or recovery metadata may replace it if tests
show the need. That strengthening should be treated as a follow-on durability
slice, not a prerequisite for the first local durable audit store.

## Storage Boundary

The audit log belongs to the local service instance. A typical layout should be
under the configured local service workspace root, for example:

```text
<service-root>/
  audit/
    security-audit.log
```

The exact path, naming, permissions, segment layout, and rotation strategy are
implementation details, but they must preserve these boundaries:

- audit files are service-owned, not workspace-owned;
- audit files are not mixed into workspace checkpoint or append-log files;
- inaccessible or unauthenticated requests can still be recorded;
- cockpit or administrative readers must be explicitly authorized later; and
- audit storage is included in future encryption-at-rest and backup decisions
  when it contains protected metadata.

## Acceptance And Recovery Semantics

For mandatory secured-profile audit, appending to the durable audit store is
authoritative only after the configured local acceptance rule succeeds. The
first local rule should be explicit about whether acceptance means write,
flush, file sync, parent-directory sync on create, or a stronger operation.

The implementation should preserve the existing local append-log recovery
discipline unless a stronger format is introduced:

- complete records are replayable retained audit input;
- a final truncated record can be treated as a torn write and ignored;
- malformed complete records make the audit store degraded or unavailable for
  secured mode;
- startup recovery reports or exposes enough state for operators and tests to
  distinguish clean recovery from degraded recovery; and
- secured effects do not resume while mandatory audit health is degraded.

This does not promise atomic transactionality between workspace state and audit
storage. Post-effect audit failure still preserves the truthful workspace
outcome and degrades audit health, as accepted in ADR 0008.

## Cockpit And Query Direction

The audit log store is the source-of-truth event list for the local service.
The cockpit can initially read, page, group, and filter this list by event
fields:

- request ID;
- service sequence;
- stage;
- authenticated principal;
- asserted actor;
- workspace;
- policy version;
- decision and reason;
- runtime outcome;
- durability outcome; and
- delivery outcome.

If later cockpit or hosted requirements need secondary indexes, richer
queries, or large-volume retention, GRM may add a projection over the audit log.
That projection may be table-like, graph-like, or external, but it must not
replace the authoritative audit log without a separate decision.

## Non-Goals

- No reuse of the workspace WAL as the audit source of truth.
- No storage of audit source-of-truth records in user graph data.
- No SQLite or general SQL event store for the first durable audit store.
- No hosted, distributed, or high-assurance audit claim.
- No tamper-evident log, signed receipt, state commitment, non-repudiation, or
  attestation.
- No external authoritative sink or forwarding protocol.
- No broad cockpit/admin API design in the storage slice.
- No cross-process writer coordination, leases, replication, or failover.
- No claim of recovery from arbitrary corruption, disk loss, or operator
  deletion.

## Consequences

Positive consequences:

- GRM keeps audit evidence on the same product path as its operational-memory
  durability primitives.
- Audit remains service-owned and independent from workspace mutation replay.
- The first durable audit store can be small, reviewable, and testable.
- A future cockpit can inspect a concrete event list without waiting for a
  hosted audit system.
- SQLite or external systems can remain later projections or forwarding
  targets rather than the first source of truth.

Tradeoffs:

- GRM owns append, sync, recovery, retention, and corruption-handling semantics.
- Querying is initially simpler than SQL and may need projections later.
- Local file semantics must be documented and tested carefully.
- The first durable store is not tamper-evident or high-assurance.
- Stronger record framing, checksums, segment recovery, and failure-injection
  tests may become necessary as audit usage grows.

## Required Proof For The First Implementation

The first implementation should be proven through public service or storage
tests that cover:

- mandatory secured-profile append success before effects;
- append failure denying pre-effect secured requests;
- post-effect append failure degrading audit health and blocking later effects;
- restart recovery of complete records;
- ignoring a torn final record or otherwise reporting it as configured;
- malformed complete record handling;
- bounded retention by count, bytes, and age;
- redaction and bounded field behavior preserved on disk;
- no storage of graph property values, credentials, raw certificates, private
  paths, policy contents, or unbounded request bodies; and
- clear separation from workspace checkpoint and append-log files.

## Relationship To Existing Decisions

This decision specializes ADR 0008's statement that future implementations may
use local durable storage. It also follows ADR 0005's direction that durable
workspace state and service-owned metadata should be explicit and recoverable,
while keeping audit separate from user graph data and workspace mutation
replay.

It does not change the current implemented truth: the present audit sink is
still process-local memory until this durable audit log store is implemented
and tested.
