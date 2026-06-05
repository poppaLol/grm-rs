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

## WorkSlice 227 Follow-Up

WorkSlice 227 added a local diagnostic Criterion group for the 1k binary
workspace persistence shape:

```sh
cargo bench --bench persistence persistence_binary_workspace_1k_breakdown -- --noplot
```

Dataset shape: 1,000 users, 1,000 posts, and 1,000 authored edges in the
embedded in-memory backend, persisted as a binary local workspace. The first
pass was collected on battery power; the later post-change pass was rerun on AC
power.

Before the load-path change, the diagnostic split showed binary reopen dominated
by in-memory decode and derived index rebuild, not filesystem read or workspace
setup:

| Diagnostic row | Battery-power local result | Interpretation |
| --- | ---: | --- |
| full binary reopen | about `8.63 ms` | Slightly slower than the anecdotal `7.6 ms`; kept as directional pre-change evidence. |
| filesystem read of primary binary file | about `23 us` | File read is not the dominant reopen cost for this local cached 1k file. |
| bincode deserialize session | about `1.61 ms` | Binary deserialization is material but not the largest cost. |
| decode JSON-encoded property values | about `1.13 ms` | The current binary format stores each property value as JSON bytes, so property decode is visible. |
| derived index rebuild | about `4.46 ms` | Dominant reopen cost before the narrow fix. |
| workspace setup | about `124 ns` | Workspace construction is noise. |
| 7-entry autocommit replay | about `108 us` | Tiny replay is cheap because it does not rebuild the full 1k graph's derived indexes. |

Checkpoint cost was split into source/store clone, binary graph projection,
bincode serialization, and full checkpoint:

| Diagnostic row | Battery-power local result | Interpretation |
| --- | ---: | --- |
| full binary checkpoint | about `5.23 ms` before the load-path change | Close to the anecdotal `4.3 ms`, with battery/noise caveat. |
| snapshot store clone | about `2.19 ms` | A real checkpoint component because the current snapshot clones source and derived store structures. |
| binary graph projection/property encode | about `1.59 ms` | A real component; property values are encoded to JSON bytes inside the binary persisted graph. |
| bincode serialize projected session | about `247 us` | Bincode serialization itself is small relative to snapshot/projection/write behavior. |

The narrow improvement changed persisted graph loads to eagerly rebuild only
label, relationship-type, and adjacency indexes, leaving the high-cardinality
node property cache dirty and rebuildable on first property-indexed read. The
post-change AC-power diagnostic showed:

| Diagnostic row | AC-power local result | Interpretation |
| --- | ---: | --- |
| full binary checkpoint | about `5.60 ms` | Checkpoint is still dominated by source snapshot/projection plus atomic write/sync/backup behavior. |
| snapshot store clone | about `1.61 ms` | A real checkpoint component because the current snapshot clones source and derived store structures. |
| binary graph projection/property encode | about `1.34 ms` | A real component; property values are encoded to JSON bytes inside the binary persisted graph. |
| bincode serialize projected session | about `209 us` | Bincode serialization itself is small relative to snapshot/projection/write behavior. |
| full binary reopen | about `4.61 ms` | Reopen improved because it no longer eagerly rebuilds node property indexes. |
| filesystem read of primary binary file | about `21 us` | File read remains negligible for this local cached 1k file. |
| bincode deserialize session | about `1.45 ms` | Binary deserialization is material but not the largest remaining cost. |
| decode JSON-encoded property values | about `1.03 ms` | Property decode remains visible because property values are JSON bytes inside the binary format. |
| eager load index rebuild | about `1.65 ms` | Remaining derived rebuild is label/relationship/adjacency work. |
| load indexes plus first property lookup | about `5.84 ms` | The property cache cost is deferred, not eliminated. |
| 7-entry autocommit replay | about `84 us` | Small replay benefits from the lighter base reopen path. |

Safe internal conclusion: disk-saved derived index contents remain a hypothesis,
not a justified next step. The current evidence supports keeping derived
indexes rebuildable and using lazy rebuild for high-cardinality property caches.
Saving derived index contents to disk should wait for a larger dataset that
shows reopen latency remains unacceptable after lazy rebuild.

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
| Partially done | Binary workspace reopen/checkpoint | Binary reopen was about `7.6 ms`; binary checkpoint was about `4.3 ms`; 7-entry autocommit replay was about `111 us`. WorkSlice 227 diagnostics now show post-change AC-power binary reopen at about `4.61 ms`, full binary checkpoint at about `5.60 ms`, and 7-entry replay at about `84 us`. | Reopen cause was narrowed to in-memory deserialize/decode plus rebuildable derived index work; eager property-index rebuild was deferred safely. Filesystem read and workspace setup are not dominant for the local cached 1k shape. Checkpoint remains partly open: source snapshot/projection plus atomic write/sync/backup behavior dominate. Disk-saved derived indexes are not justified yet. |
| 3 | Embedded write operation scaling | Populated-state create/update node and edge operations grew from roughly `12-19 us` at 1k to roughly `29-38 us` at 10k. | This suggests possible size-sensitive work, perhaps index invalidation, validation, lookup, or derived structure maintenance. Investigate after persistence unless the product sequence switches to TLS first. |
| 4 | Bulk insert versus SQLite | At 1k, GRM bulk insert was about `5.6 ms`; SQLite in-memory transaction was about `2.7 ms`. | Expected comparator weakness: GRM does graph-specific bookkeeping that pays for fast graph reads. Understand the cost, but do not optimize by weakening graph correctness or derived-structure invariants. |
| 5 | Local insecure gRPC per-call overhead | Local insecure gRPC calls generally landed around `115-225 us`, while embedded equivalents were often sub-microsecond to tens of microseconds. | Expected transport/workspace overhead. Keep as a local overhead baseline. This may matter for batching and service ergonomics, but public service comparisons wait for TLS. |

## Priority Order

The agreed sequence after WorkSlice 227 is:

1. Move next to the narrow TLS-capable gRPC service slice. This is the product
   sequencing choice, so public client/server comparator claims can eventually
   rest on a credible secured GRM service line.
2. Circle back to performance after the TLS slice, starting with embedded write
   operation scaling unless new TLS evidence changes the order.
3. Bulk insert cost versus SQLite.
4. Local insecure gRPC per-call overhead.
5. Larger-dataset persistence follow-up only if reopen/checkpoint latency
   remains important after the lazy property-index rebuild change.

TLS remains required before public client/server comparator claims. The local
performance threads are parked, not abandoned.

## Safe Claims

Safe internal claim:

- The baseline artifacts identify likely next investigations for WorkSlice 205.
- WorkSlice 226 identified and fixed the dominant warmed embedded selective
  traversal scaling cause; raw traversal acceleration should wait.
- WorkSlice 227 identified binary reopen cost centers, applied lazy
  property-index rebuild for persisted graph loads, and found disk-saved derived
  index contents unjustified for the current local 1k evidence.

Unsupported claims:

- GRM service performance against Postgres, Mongo, Neo4j, or other client/server
  databases.
- Hosted durability, multi-writer behavior, production security, or TLS
  overhead.
- GraphBLAS, traversal acceleration, or public service/database performance
  claims from this local embedded diagnostic.
