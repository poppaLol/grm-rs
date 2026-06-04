# Performance Baseline Anecdotal Results

This note records an initial local read of Criterion artifacts under
`target/criterion/*` during WorkSlice 205. Treat it as anecdotal engineering
memory, not a repeatable public benchmark report.

The results were useful for choosing the next investigation order, but they do
not yet include the full provenance envelope planned for the future repeatable
cloud/VPS benchmark runner. Do not use this note for public service/database
comparison claims.

## WorkSlice 226 Follow-Up

WorkSlice 226 investigated the first pain point and found that raw embedded
traversal execution is effectively flat for the selective 1k and 10k graph
shapes after the executor candidate-selection fix.

The dominant scaling cause was not GraphBLAS-worthy traversal execution,
end-filter handling, profile instrumentation, or a bad explain planner choice.
The in-memory graph executor was materializing label candidates before using the
more selective label+property candidate set. Reordering candidate selection to
try exact property candidates before falling back to label candidates removed
the visible raw traversal scaling.

Current local diagnostic measurements from
`cargo bench --bench grm_vs_sqlite embedded_traversal_breakdown -- --noplot`
showed:

| Diagnostic row | 1k | 10k | Interpretation |
| --- | ---: | ---: | --- |
| raw graph execution | about `3.07 us` | about `3.22 us` | Selective traversal execution is flat in the warmed embedded path. |
| `node.find` traversal with end filter | about `8.15 us` | about `10.04 us` | Remaining delta is small wrapper/post-filter/materialization cost. |
| traversal explain | about `32.10 us` | about `31.51 us` | Planner/explain cost is fixed for this shape. |
| traversal profile | about `54.47 us` | about `68.19 us` | Profile remains dominated by public introspection/reporting overhead and Criterion variance. |

Internal traversal profile phase timings over warmed embedded fixtures were also
flat: `explain` about `7 us`, `anchor_metric` about `1 us`,
`execute_node_query` about `12-14 us`, `metric_push` below `1 us`, and
`profile_value` about `42-43 us`. A cold first profile call can still pay lazy
property-index rebuild in `anchor_metric`, so the diagnostic interpretation is
for warmed steady-state profile behavior.

Safe internal conclusion: traversal acceleration should wait. The next
performance investigation should move to binary workspace reopen/checkpoint
cause analysis unless new evidence changes the order.

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
| Done | Selective traversal and `node.find` profile scaling | Initial artifacts showed embedded selective traversal moving from about `13 us` at 1k to about `84 us` at 10k, and embedded `profile_node_find` from about `63 us` at 1k to about `171 us` at 10k. WorkSlice 226 diagnostics now show warmed raw graph execution at about `3.07 us` for 1k and `3.22 us` for 10k after the candidate-selection fix. | Cause identified and narrow fix applied: candidate selection was materializing label candidates before the selective label+property index path. Profile overhead is mostly fixed introspection/reporting. Traversal acceleration should wait. |
| 2 | Binary workspace reopen/checkpoint | Binary reopen was about `7.6 ms`; binary checkpoint was about `4.3 ms`; 7-entry autocommit replay was about `111 us`. | Tiny replay is fine, but full reopen/checkpoint need cause analysis. Possible causes include deserialization, schema/catalog rebuild, derived index rebuild, validation, or filesystem write/read behavior. Disk-saved derived indexes may become relevant only after cause is known. |
| 3 | Embedded write operation scaling | Populated-state create/update node and edge operations grew from roughly `12-19 us` at 1k to roughly `29-38 us` at 10k. | This suggests possible size-sensitive work, perhaps index invalidation, validation, lookup, or derived structure maintenance. Investigate after traversal/profile and persistence cause analysis unless evidence changes. |
| 4 | Bulk insert versus SQLite | At 1k, GRM bulk insert was about `5.6 ms`; SQLite in-memory transaction was about `2.7 ms`. | Expected comparator weakness: GRM does graph-specific bookkeeping that pays for fast graph reads. Understand the cost, but do not optimize by weakening graph correctness or derived-structure invariants. |
| 5 | Local insecure gRPC per-call overhead | Local insecure gRPC calls generally landed around `115-225 us`, while embedded equivalents were often sub-microsecond to tens of microseconds. | Expected transport/workspace overhead. Keep as a local overhead baseline. This may matter for batching and service ergonomics, but public service comparisons wait for TLS. |

## Priority Order

The agreed investigation order is:

1. Binary workspace reopen/checkpoint cause analysis.
2. Embedded write operation scaling.
3. Bulk insert cost versus SQLite.
4. Local insecure gRPC per-call overhead.

The project may switch to the narrow TLS-capable gRPC service slice before
items 2, 3, or 4. TLS remains required before public client/server comparator
claims.

## Safe Claims

Safe internal claim:

- The baseline artifacts identify likely next investigations for WorkSlice 205.
- WorkSlice 226 identified and fixed the dominant warmed embedded selective
  traversal scaling cause; raw traversal acceleration should wait.

Unsupported claims:

- GRM service performance against Postgres, Mongo, Neo4j, or other client/server
  databases.
- Hosted durability, multi-writer behavior, production security, or TLS
  overhead.
- GraphBLAS, traversal acceleration, or public service/database performance
  claims from this local embedded diagnostic.
