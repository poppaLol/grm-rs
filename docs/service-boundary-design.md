# Service Boundary Design Spike

This document captures the intended service boundary for GRM after Shared WAL
durability core changes. It began as a docs-only design spike; the current
codebase now contains a split-ready `grm-service-api` contract crate with
generated protobuf DTOs, in-process mappings into the existing runtime
dispatcher, and a minimal local gRPC workspace shell. The goal remains to make
the future hosted/service contract concrete enough that security work, daemon
work, SDKs, and packaging can be designed against it without prematurely
claiming a production service.

See also [ADR 0001: Separate Graph Data From Schema Memory](adr/0001-graph-data-and-schema-memory.md)
for the product architecture principle that future backends should store user
graph data separately from schema memory that communicates and validates the
intended shape of that graph.

See also [ADR 0004: Frame GRM As A Structured Operational Memory Layer](adr/0004-structured-operational-memory-layer.md)
for the accepted framing that GRM's product surface is operational memory
semantics rather than a traditional graph database or CRUD API.

## Product Position

GRM's sellable surface is a Structured Operational Memory Layer:

> typed, secure, explainable operational memory for applications and agents

It is not:

> learn our database query dialect

The future service contract should be typed operational memory requests over
gRPC/protobuf. CLI command text can remain a human adapter. Python helpers, MCP
tools, and future SDKs can remain ergonomic wrappers. The service boundary
itself should receive structured request objects for schema/session operations,
node and edge mutation where appropriate, batch deltas, traversal/state
resolution, projections, explain/profile, and durability/admin work.

This keeps the trusted boundary aligned with GRM's core value: applications and
agents send typed operational memory requests, not arbitrary script text.

## Client Surface Endgame

The gRPC/protobuf contract is expected to become the default persisted
operational memory layer for GRM. CLI, Python, MCP, Rust clients, generated
SDKs, and future C#/TypeScript surfaces should be able to run as clients of the
service when users want a shared or durable workspace.

Embedded runtime use remains valid for local utilities, tests, scripts, and
lightweight applications. The distinction should be deployment and durability
class, not behavior: embedded adapters and service clients should issue the same
typed workspace/runtime operations wherever practical.

Neo4j remains an optional graph-data backend and inspection/interoperability
path. It may be useful indefinitely, but it should not be treated as GRM's
default persisted SOML layer. The service-backed workspace is where GRM should
eventually own schema memory, durable deltas/checkpoints, policy, auth, audit,
observability, and future provenance as one operational memory envelope.

## Protocol Choice

gRPC/protobuf should be the canonical service protocol.

Reasons:

- protobuf defines naturally typed request and response messages
- client generation is strong across Rust, Python, TypeScript, Go, Java, C#,
  and other common application languages
- gRPC fits certificate-based service authentication, including production mTLS
- OpenTelemetry trace context can flow through gRPC metadata, and RPC metrics
  can follow standard semantic conventions
- request validation and authorization can operate on typed fields instead of
  parsed command strings
- streaming RPCs can be added later for profile events, watches, WAL replay,
  backup/restore progress, and recovery reporting

An HTTP/JSON gateway may be useful later for demos, marketplace ergonomics,
simple hosted examples, or environments where gRPC tooling is inconvenient.
That gateway should be an adapter over the protobuf contract, not the
source-of-truth service API.

## Future HTTP Admin/UI Layer

A future browser-based admin UI may expose HTTP and WebSocket endpoints for
human workflows. That layer could include a command console, graph explorer,
schema browser, explain/profile views, durability status, backup/restore
progress, and telemetry dashboards.

This UI layer should remain an adapter over the typed gRPC/protobuf service
contract. Even if the browser experience includes text input for convenience,
the trusted backend boundary should still receive typed service requests rather
than treating browser command text as the canonical API.

## Ports And Deployment Shape

Local daemon defaults can be developer-friendly, such as binding gRPC to
`127.0.0.1:50051` for explicit local/dev service mode. External binding should
be explicit rather than accidental.

Production deployments should make host, port, TLS, and certificate settings
configurable. A hosted GRM service may commonly sit behind TLS on port `443`,
either directly or through an ingress/proxy/load balancer. Metrics may be
exposed separately, exported through OpenTelemetry collector configuration, or
both, depending on deployment shape.

Insecure local/dev modes should be visibly unsafe escape hatches. They should
not become the default for production-like daemon or hosted configurations.

## Service Surface Sketch

The first `.proto` package layout now exists in `grm-service-api`. It organizes
the service boundary around explicit operation families and codegen-checked
protobuf files. The crate includes a local gRPC workspace shell for
create/open/execute/close workspace RPCs, but deliberately stops short of a
daemon, TLS, auth, authorization, hosted durability, or direct implementation of
every RPC family.

### SchemaService

Candidate RPCs:

- `DefineNodeModel`
- `DefineEdgeModel`
- `ListSchema`
- `DescribeModel`

This service owns runtime schema definition and inspection. It should expose
node models, edge/link models, field names, field value types, required flags,
ID field metadata, and endpoint constraints for edge models.

### NodeService

Candidate RPCs:

- `CreateNode`
- `UpdateNode`
- `DeleteNode`
- `FindNodes`

Node requests should carry the model name, typed property values, IDs where
needed, optional predicates, ordering, limit, and offset. `FindNodes` should
remain a structured request rather than a textual `node.find` command.

### EdgeService

Candidate RPCs:

- `CreateEdge`
- `UpdateEdge`
- `DeleteEdge`
- `FindEdges`

Edge requests should carry the edge model/link name, endpoint IDs for create,
edge IDs for update/delete, typed property values, optional endpoint filters,
predicates, ordering, limit, and offset.

### BatchService

Candidate RPC:

- `ApplyGraphPatch`

`ApplyGraphPatch` should accept:

- an `atomic` flag
- an explicit `allow_deletes` or equivalent delete safety control
- ordered typed operations for schema, node, and edge changes
- optional client-provided operation references for linking newly created nodes
- response shape controls for summary versus detailed results

The durable operation grouping should remain explicit. A successful atomic
patch maps naturally to one durable grouped operation, while non-atomic patches
may expose per-operation success and failure semantics.

Current implementation note: generated protobuf batch DTOs can be converted into
the service/runtime request shape and executed in-process through
`SessionState::execute_runtime`, which routes batch requests to the existing
`apply_session_batch` path. The response preserves the runtime batch value,
`should_persist`, and grouped durable operation metadata.

### QueryService

Candidate RPCs:

- `NodeFind`
- `EdgeFind`
- `Traversal`

These are structured query requests, not arbitrary textual query strings.
Traversal should be represented as constrained protobuf messages: root model,
root predicates, ordered traversal steps, direction, edge model, end model,
optional edge/end predicates, return mode, ordering, and limits.

The important design constraint is that traversal objects must not grow into a
hidden scripting language. If GRM later adds richer query capabilities, they
should remain bounded typed constructs with clear validation, authorization,
metering, and explainability.

### IntrospectionService

Candidate RPCs:

- `Explain`
- `Profile`
- `IndexCatalog`
- `DescribeSession`
- `DescribeBackend`

`Explain` should return logical plan shape and access-path metadata without
executing the request. `Profile` should execute under explicit cost limits and
return row counts, timing, plan shape, index/access-path metadata, and execution
status.

`IndexCatalog` should expose derived backend-maintained metadata, not imply that
user-defined indexes are already part of the product. `DescribeSession` and
`DescribeBackend` should expose capabilities, backend identity type, durability
mode, and version information useful for SDKs and operators.

### DurabilityAdminService

Candidate RPCs:

- `Checkpoint`
- `Recover` / `Open`
- `Compact`
- `WalStatus`
- `Backup` / `Restore` later

This service should stay operationally explicit. Durability/admin RPCs need
stricter permissions and request limits than normal graph reads and writes.
Backup/restore can come later; the first design should still reserve space for
streaming progress and failure reporting.

## Non-Goals

- No cloud service implementation in this PR.
- No TLS or certificate implementation in this PR.
- No authorization engine implementation in this PR.
- No textual query language at the service boundary.
- No distributed or multi-writer durability claim.
- No clustering or peering implementation in this PR.

The current embedded runtime should not be reshaped just to satisfy this spike.
This document is meant to guide the next API-design PR, not force service
infrastructure into the codebase early.

## Future Clustering And Peering

If GRM later adds clustering, replication, leader election, peering, or remote
durability coordination, public client APIs and internal peer APIs should be
separate service families. They should have separate protobuf packages or
service namespaces, separate authorization policy, and separate certificate
identity expectations.

Public gRPC APIs should represent application and agent operations. Internal
peer gRPC APIs should represent node-to-node coordination and should not inherit
client-facing permissions by accident.

## Security Implications

Typed gRPC is generally easier to secure than a scripting or query language, but
it is not secure automatically.

Advantages over a textual scripting/query boundary:

- typed messages are easier to validate before execution
- authorization can be attached to operation families such as schema, node CRUD,
  edge CRUD, batch patch, query/traversal, explain/profile, and admin
- request metering can inspect structured fields such as batch length,
  traversal depth, result limit, requested profile detail, and target model
- audit records can capture operation type, model/link name, IDs, request ID,
  client identity, and later actor/tenant
- parser injection is not the primary trusted-boundary risk
- there are no arbitrary expressions to sandbox by default

Production service mode should default to encrypted transport. The expected
production path is certificate-based service authentication, preferably mTLS for
service-to-service deployments. Local/dev insecure modes may exist, but they
must be explicit, visibly unsafe escape hatches rather than silent defaults.

Authorization should be designed before marketplace packaging. The permission
model should distinguish at least:

- schema operations
- node create, read/find, update, and delete
- edge create, read/find, update, and delete
- batch patch
- query/traversal
- explain/profile
- durability/admin operations

Request limits should be first-class service policy, not scattered defensive
checks. Initial policy dimensions should include:

- maximum batch operation count
- maximum traversal depth
- maximum result size and default page size
- maximum profile cost and profile detail level
- WAL append pressure and checkpoint pressure
- maximum serialized request and response size
- per-client or per-actor concurrency limits later

Auditability should be designed into request handling from the start. A useful
audit event should include operation family, RPC name, model or link name, IDs
when present, request ID, status, latency, and later actor, client, tenant, and
authorization decision metadata.

## OpenTelemetry Implications

gRPC is compatible with trace propagation through request metadata and with
standard RPC semantic conventions for spans and metrics. A future daemon should
create spans around service request handling and propagate context into runtime,
durability, and backend work.

Future spans and metrics should cover:

- request latency and status by RPC and operation family
- actor/client identity later, recorded carefully as low-cardinality attributes
  or linked audit metadata
- WAL append latency, bytes, failures, and queue/pressure signals
- checkpoint latency, bytes, and failures
- recovery events, replay count, skipped/truncated WAL records, and duration
- query/profile execution timing, rows scanned, rows returned, and status
- index usage and access-path metadata from explain/profile
- backend call timing and backend error classification

The design should keep observability useful without leaking sensitive graph data
into high-cardinality span attributes.

## Relationship To Existing Code

The current runtime already points in the right direction. `RuntimeRequest` is a
typed umbrella over request families that can map naturally onto future protobuf
messages:

- `RuntimeRequest` maps to the union of service operation families.
- `SchemaRequest` maps to `SchemaService` requests such as `DefineNodeModel` and
  `DefineEdgeModel`.
- `NodeRequest` maps to `NodeService` create/update/delete/find requests.
- `EdgeRequest` maps to `EdgeService` create/update/delete/find requests.
- `BatchRequest` maps to `BatchService.ApplyGraphPatch`.
- `QueryRequest` maps to `QueryService.NodeFind`, `QueryService.EdgeFind`, and
  `QueryService.Traversal`.
- `ExplainRequest` maps to `IntrospectionService.Explain`.
- `ProfileRequest` maps to `IntrospectionService.Profile`.
- `AdminRequest` maps partly to `IntrospectionService` and partly to
  `DurabilityAdminService`.
- `DurableOperation` maps to the internal durable operation log and to future
  durable grouping semantics for service-side mutation commits.

CLI text syntax should remain an adapter that parses human commands into typed
requests. Python bindings should continue to expose ergonomic method calls over
the same request semantics. MCP tools should do the same for agent workflows.
A future daemon should converge on these typed request semantics rather than
introducing a separate textual service language.

This convergence matters because it gives GRM one core behavior contract across
embedded Rust, CLI, Python, MCP, and hosted/service mode.

## Current Service API Progress

The near-term service API proof has moved from design into code:

- `grm-service-api` contains the initial typed protobuf files for schema, node,
  edge, batch, query, introspection, and durability/admin messages.
- The crate generates Rust DTOs from those protobuf definitions at build time.
- Generated protobuf schema and batch requests are converted into typed service
  request shapes and executed through the existing runtime dispatcher in tests.
- Runtime dispatcher batch support now reuses the existing batch implementation
  rather than introducing a new service-only mutation path.
- A minimal local gRPC workspace shell exposes create/open/execute/close
  workspace RPCs and delegates execution through `InProcessWorkspaceService`
  using managed workspace handles, snapshot handles, and opaque workspace refs.
- The local shell can map opaque workspace refs to autocommit workspace files
  beneath a server-configured root, so generated clients can prove
  create/open/execute/reopen behavior without passing server filesystem paths.
- Generated-client parity coverage now targets the practical MCP-Neo4j subset
  through the workspace path: schema define/list, schema-aware node and edge
  CRUD, simple find, traversal-backed find result shapes, explain/profile for
  typed find shapes through service-backed adapters, batch, and reopen
  verification.
- Unsupported runtime surfaces remain explicit: free-form query parity, admin
  operations, and direct non-workspace RPCs are not silently claimed as
  implemented.

This is a service-hostable runtime contract proof with a thin local gRPC
transport shell, not a hosted service. The next service-boundary work should
avoid widening into daemon lifecycle, auth, authorization, hosted durability
claims, multi-writer coordination, or direct RPC implementations until request
context, durability behavior, and unsupported semantics are clear enough to
review. A narrow TLS-capable service path is expected before public
service/database benchmark comparisons, but that TLS slice should stay scoped
to credible transport measurement and should not imply auth/RBAC, certificate
lifecycle, hosted durability, or production service readiness.
The current local durability target class is documented separately in
[Local Durability Target Class](local-durability-target.md).
