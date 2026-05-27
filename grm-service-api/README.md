# GRM Service API

`grm-service-api` is the first split-ready home for GRM's future typed service
contract. It lives in the monorepo per ADR 0002, but the crate is deliberately
client-facing: it contains protobuf source files, generated DTOs, and typed
conversion helpers, not daemon internals.

The proto skeleton mirrors the current structured runtime boundary:

- `RuntimeRequest` maps to schema, node, edge, query, explain/profile, batch,
  and durability/admin request families.
- `RuntimeResponse` maps to family-specific response messages.
- `RuntimeDispatchOutcome::durable_ops` maps to `DurableMutationOutcome` on
  write responses. Read responses omit durable mutation fields.

Generated protobuf DTOs are build-checked and can be converted into the
service/runtime request shape. Tests prove generated schema and batch requests
can execute through `SessionState::execute_runtime`; batch execution reuses the
existing runtime batch path and preserves grouped durable operation metadata.
A minimal local gRPC shell exposes the workspace RPCs over the same generated
contract and delegates to `InProcessWorkspaceService`. The shell can also bind
opaque `WorkspaceRef` values to local autocommit workspace files under a
server-configured root, letting generated clients create, execute, close, and
reopen a durable local workspace without sending server filesystem paths.

The contract does not expose CLI command text as a query surface. Query,
traversal, explain, and profile requests are typed request messages.

Durability/admin messages avoid client-supplied server filesystem paths. The
public skeleton uses managed snapshot handles and bytes for import/export; local
path-based CLI behavior remains an adapter concern.

This crate does not implement a daemon, choose hosted transport/TLS/auth policy,
or add new graph mutation/query semantics. Direct non-workspace RPCs remain
explicit unsupported surfaces in the local gRPC shell until implemented and
tested.
