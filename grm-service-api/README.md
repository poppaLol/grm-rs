# GRM Service API

`grm-service-api` is the first split-ready home for GRM's future typed service
contract. It lives in the monorepo per ADR 0002, but the crate is deliberately
client-facing: it contains protobuf source files and tiny Rust helpers for
locating them, not daemon internals.

The proto skeleton mirrors the current structured runtime boundary:

- `RuntimeRequest` maps to schema, node, edge, query, explain/profile, batch,
  and durability/admin request families.
- `RuntimeResponse` maps to family-specific response messages.
- `RuntimeDispatchOutcome::durable_ops` maps to `DurableMutationOutcome` on
  write responses. Read responses omit durable mutation fields.

The contract does not expose CLI command text as a query surface. Query,
traversal, explain, and profile requests are typed request messages.

Durability/admin messages avoid client-supplied server filesystem paths. The
public skeleton uses managed snapshot handles and bytes for import/export; local
path-based CLI behavior remains an adapter concern.

This crate does not implement a daemon, choose transport/TLS/auth policy, or add
new graph mutation/query semantics.
