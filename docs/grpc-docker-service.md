# GRM gRPC Docker Service

This is an insecure Docker-hostable demo for the local GRM gRPC workspace shell.
It is intended for local development, examples, and adapter integration tests.
It is not a production daemon and does not provide TLS, authentication,
authorization, hosted durability, or multi-writer coordination.

The container runs the `grm-service-api` `local_workspace_server` example:

```text
grm-local-workspace-server 0.0.0.0:50051 /workspaces
```

`/workspaces` is a mounted workspace root. Workspace refs sent by clients are
mapped by the service to local autocommit workspace files under that root.
Clients do not send server-local filesystem paths.

## Supported Surface

The local shell exposes the `grm.service.v1.GrmService` protobuf service. The
supported path is workspace-scoped:

- `CreateWorkspace`
- `OpenWorkspace`
- `ExecuteWorkspace`
- `CloseWorkspace`

Use `ExecuteWorkspace` for schema, node, edge, simple find, and batch runtime
requests. The direct non-workspace RPC families in the proto are placeholders in
the current local shell and return explicit unsupported errors.

Current non-goals:

- production daemon lifecycle
- TLS/mTLS or authentication
- authorization, quotas, request limits, and audit
- hosted durability or multi-writer guarantees
- direct RPC-family parity outside `ExecuteWorkspace`
- full traversal/query/explain/profile/import/export parity through all adapters

## Run With Docker Compose

```bash
docker compose up --build
```

The service listens on `localhost:50051`. Workspace data is stored in the named
Docker volume `grm-workspaces`.

Stop the service:

```bash
docker compose down
```

Remove the demo workspace volume:

```bash
docker compose down -v
```

## Run The Rust Client Example

In another shell:

```bash
cargo run -p grm-service-api --example local_workspace_client -- \
  http://127.0.0.1:50051 docker-demo
```

The example creates or opens the `docker-demo` workspace through the gRPC
service, defines schema, creates nodes and edges, performs finds and batch
operations, closes the workspace, reopens it, and verifies data is still present.

## Inspect With grpcurl

The server does not currently enable gRPC reflection, so `grpcurl` calls must
include the local proto files. To avoid host-specific native `grpcurl`
packaging issues, run the published `grpcurl` container on the Compose network:

```bash
docker run --rm \
  --network grm-rs_default \
  -v "$(pwd)/grm-service-api/proto:/protos:ro" \
  fullstorydev/grpcurl:latest \
  -plaintext \
  -import-path /protos \
  -proto grm/service/v1/service.proto \
  grm-grpc:50051 \
  grm.service.v1.GrmService/CreateWorkspace
```

See [grpc-quickstart.md](grpc-quickstart.md) for a small `grpcurl` walkthrough.

## Dogfood Neo4j MCP Memory Migration

To copy a Neo4j-backed MCP graph into a gRPC workspace for dogfooding, use the
checked Rust migration example. It reads the session-local schema memory file
used by Neo4j MCP mode, reads matching typed nodes and edges from Neo4j, then
replays them into the workspace service through `ExecuteWorkspace`.

```bash
NEO4J_URI=bolt://localhost:7687 \
NEO4J_USER=neo4j \
NEO4J_PASSWORD=... \
cargo run -p grm-service-api --example migrate_neo4j_to_grpc -- \
  --schema /path/to/project-memory-schema.json \
  --endpoint http://127.0.0.1:50051 \
  --workspace project-memory-grpc \
  --mode create
```

Then restart MCP against the migrated workspace:

```bash
GRM_BACKEND=grpc
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051
GRM_WORKSPACE_REF=project-memory-grpc
GRM_SERVICE_WORKSPACE_MODE=open
```

This is an experiment, not a lossless backup tool. It remaps Neo4j node IDs to
new workspace-local IDs and currently requires schema labels and relationship
types to match their GRM model names.

## Security And Durability Notes

This demo uses local autocommit workspace files in the container volume. That is
a tested local workflow, not a hosted durability claim. Treat the container as a
single-writer local service process.

Before using this shape beyond local development, GRM still needs explicit
service lifecycle, TLS/auth, authorization, limits, audit, recovery, and
coordination design.
