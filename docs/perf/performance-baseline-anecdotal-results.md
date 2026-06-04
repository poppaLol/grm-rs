# Performance Baseline Anecdotal Results

This note records an initial local read of Criterion artifacts under
`target/criterion/*` during WorkSlice 205. Treat it as anecdotal engineering
memory, not a repeatable public benchmark report.

The results were useful for choosing the next investigation order, but they do
not yet include the full provenance envelope planned for the future repeatable
cloud/VPS benchmark runner. Do not use this note for public service/database
comparison claims.

## Scope

The artifacts cover current local Criterion benchmark groups for:

- embedded GRM baseline runtime operations
- local insecure gRPC workspace operations
- SQLite local comparator rows
- binary workspace checkpoint, reopen, and small-log replay
- existing insert, property lookup, one-hop traversal, and transaction-overlay
  read groups

Local insecure gRPC remains a local transport-overhead and demo line only. It is
not a credible deployable service baseline until a TLS-capable GRM service line
exists.

## Observed Pain Points

| Priority | Area | Anecdotal signal | Interpretation |
| --- | --- | --- | --- |
| 1 | Selective traversal and `node.find` profile scaling | Embedded selective traversal moved from about `13 us` at 1k to about `84 us` at 10k. Embedded `profile_node_find` moved from about `63 us` at 1k to about `171 us` at 10k. | This is the best first engine investigation because it is graph-native, user-visible, and tied to explain/profile as the acceleration-orientation surface. |
| 2 | Binary workspace reopen/checkpoint | Binary reopen was about `7.6 ms`; binary checkpoint was about `4.3 ms`; 7-entry autocommit replay was about `111 us`. | Tiny replay is fine, but full reopen/checkpoint need cause analysis. Possible causes include deserialization, schema/catalog rebuild, derived index rebuild, validation, or filesystem write/read behavior. Disk-saved derived indexes may become relevant only after cause is known. |
| 3 | Embedded write operation scaling | Populated-state create/update node and edge operations grew from roughly `12-19 us` at 1k to roughly `29-38 us` at 10k. | This suggests possible size-sensitive work, perhaps index invalidation, validation, lookup, or derived structure maintenance. Investigate after traversal/profile and persistence cause analysis unless evidence changes. |
| 4 | Bulk insert versus SQLite | At 1k, GRM bulk insert was about `5.6 ms`; SQLite in-memory transaction was about `2.7 ms`. | Expected comparator weakness: GRM does graph-specific bookkeeping that pays for fast graph reads. Understand the cost, but do not optimize by weakening graph correctness or derived-structure invariants. |
| 5 | Local insecure gRPC per-call overhead | Local insecure gRPC calls generally landed around `115-225 us`, while embedded equivalents were often sub-microsecond to tens of microseconds. | Expected transport/workspace overhead. Keep as a local overhead baseline. This may matter for batching and service ergonomics, but public service comparisons wait for TLS. |

## Priority Order

The agreed investigation order is:

1. Selective traversal and `node.find` profile scaling.
2. Binary workspace reopen/checkpoint cause analysis.
3. Embedded write operation scaling.
4. Bulk insert cost versus SQLite.
5. Local insecure gRPC per-call overhead.

The project may switch to the narrow TLS-capable gRPC service slice before
items 3, 4, or 5. TLS remains required before public client/server comparator
claims.

## Safe Claims

Safe internal claim:

- The baseline artifacts identify likely next investigations for WorkSlice 205.

Unsupported claims:

- GRM service performance against Postgres, Mongo, Neo4j, or other client/server
  databases.
- Hosted durability, multi-writer behavior, production security, or TLS
  overhead.
- Any acceleration target before the first investigation confirms cause.

