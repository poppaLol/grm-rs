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

GRM uses a service-owned GRM-native audit log store as the durable
local persistence direction for security audit events.

The audit log store is a distinct service storage component. It is not the
workspace WAL, not user graph state, not Neo4j project memory, and not an
external monitoring system. It stores a bounded append-oriented ordered list of
`SecurityAuditEvent` records under service-owned storage, with recovery and
retention semantics designed for local secured-service audit evidence.

The authoritative representation is a list of typed immutable events, not an
ordinary GRM user graph. Stable typed identifiers and service ordering carry
correlation. Graph-like relationships may be derived later for cockpit use,
but are rebuildable views and never participate in append acceptance.

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
    store-metadata
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

For mandatory secured-profile audit, the local rule is "file plus directory
acceptance": the complete versioned record and newline are written,
`File::sync_all()` succeeds, and a newly created log is made durable by syncing
the audit directory. Initialization likewise syncs the metadata file, its
audit-directory entry, and a newly created audit-directory entry through the
service root. `append` returns `Ok` only after the operations required for that
append succeed.

Metadata contains a random 128-bit hexadecimal store-generation ID. Normal
reopen recovers it; a newly initialized store receives another ID; malformed,
unsupported, or record-conflicting metadata fails closed. Event and request
identity are `(store_generation_id, event_id)` and
`(store_generation_id, request_id)`.

Counters use synchronized bounded range reservations plus separate accepted
high-water marks. Request IDs reserve 64 identifiers at a time. Event IDs and
service sequences reserve their corresponding 64-identifier ranges together in
one atomic metadata replacement. A range is atomically replaced and
directory-synced before any identifier in it is issued, so a crash may lose at
most 64 identifiers per counter at a reservation boundary but cannot reuse an
issued identifier. Accepted high-water fields advance only after the event
record is durable. Recovery validates ordering, starts above the durable range
ceilings even when compaction removed older records, detects exhaustion, and
continues service sequence strictly above recovered history.

For a seven-stage request starting with empty reservation ranges, the original
per-identifier design required 22 metadata replacements: one request-ID
reservation, fourteen separate event/sequence reservations, and seven accepted
high-water updates. The bounded combined-range design requires nine: one
request range, one combined event/sequence range, and seven accepted updates.
Within an existing range it requires only the seven accepted updates. Event
file and directory acceptance synchronization is unchanged.

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

Before a degraded mandatory service requires restart, the service performs one
bounded in-process recovery probe before permitting the next protected effect.
The probe re-reads and validates metadata version and generation, all complete
records, bounded event fields, ordering, reservation ceilings, and retention.
It repairs a torn final record or retention change through the same synchronized
temporary-file compaction path and re-synchronizes the audit directory. The
active log is explicitly file-synchronized when compaction is unnecessary;
recovered complete records advance accepted high-water metadata through the
normal synchronized metadata-replacement protocol. The
in-memory retained view and counter positions are replaced only after every
validation, repair, and synchronization succeeds. Any inconsistency or storage
failure leaves both sink and service degraded and secured effects fail closed.

Audit timestamps are service-authored wall-clock evidence, not a trusted
monotonic clock or tamper-proof chronology. Append rejects pre-epoch timestamps
and excessive newly authored future skew. Recovery does not reject an otherwise
valid record merely because wall-clock rollback makes its timestamp appear in
the future. Such records retain their original timestamp, are reported through
a bounded future-record count, and do not expire by age until wall time catches
up. Count and retained-byte limits still apply normally.

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

## Retention And Physical Replacement

Events are immutable while retained and ordinary acceptance appends complete
records. Count, serialized retained bytes, age, and individual record size are
finite. When retention removes an event, the store writes the retained suffix
to a temporary file, syncs it, atomically renames it over the active log, and
syncs the audit directory. Only then is compaction committed. Failure before
rename leaves the previous valid log; failure after rename but before directory
sync is reported as unavailable.

The physical file is therefore not permanently append-only. Bounded retention
may atomically replace it. The honest guarantee is a bounded append-oriented
authoritative event list whose retained events are immutable; intentionally
expired events are outside the retention guarantee.

The supported scope is one service process, one writer, and an honest local
filesystem implementing the tested Rust file and directory synchronization
operations. A process-local ownership guard rejects a second store instance for
the same canonical audit directory. There is still no cross-process file lock
or fencing: operators must ensure only one process opens a store. Two processes
can otherwise race on metadata and compaction temporary paths. There is no recovery claim for arbitrary
corruption, disk loss, hostile replacement, lying controllers, or operator
deletion.

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

The local service binary initializes this store under its configured service
root. Explicit injected sinks remain available for bounded tests and alternate
deployment wiring; anonymous-local remains best-effort even when its local sink
happens to be durable.
