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
`GrpcWorkspaceClient` also exposes Rust-facing helpers for the checked
workspace subset, so callers can use `define_node`, `schema_list`,
`create_node`, `find_nodes` for node-only results, `find_node_results` for
node.find node-or-edge results, `create_edge`, `find_edges`, and `apply_batch`
without assembling generated protobuf messages by hand. The generated proto
module remains public for lower-level callers.
A minimal local gRPC shell exposes the workspace RPCs over the same generated
contract and delegates to `InProcessWorkspaceService`. The shell can also bind
opaque `WorkspaceRef` values to local autocommit workspace files under a
server-configured root, letting generated clients create, execute, close, and
reopen a durable local workspace without sending server filesystem paths.
Generated-client and ergonomic-client tests also exercise the practical
MCP-Neo4j CRUD parity subset through `ExecuteWorkspace`: schema define/list,
schema-aware node and edge CRUD, simple find, batch, and reopen verification.

The contract does not expose CLI command text as a query surface. Query,
traversal, explain, and profile requests are typed request messages.

Durability/admin messages avoid client-supplied server filesystem paths. The
public skeleton uses managed snapshot handles and bytes for import/export; local
path-based CLI behavior remains an adapter concern.

This crate does not implement a daemon, choose hosted transport/TLS/auth policy,
or add new graph mutation/query semantics. Direct non-workspace RPCs remain
explicit unsupported surfaces in the local gRPC shell until implemented and
tested.

Run the local workspace shell and generated-client walkthrough with:

```bash
cargo run -p grm-service-api --example local_workspace_server -- 127.0.0.1:50051 /tmp/grm-service-workspaces
cargo run -p grm-service-api --example local_workspace_client -- http://127.0.0.1:50051 demo-workspace
```

The checked service-backed client path uses binary local autocommit workspace
files by default. JSON remains available when a caller explicitly requests the
JSON durability format. The supported durability target is single-writer local
filesystem behavior for service-managed workspaces, not hosted durability or
multi-writer coordination; see
[Local Durability Target Class](../docs/local-durability-target.md).
