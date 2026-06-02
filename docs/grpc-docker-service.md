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
Clients do not send server-local filesystem paths. New service-backed client
usage defaults to binary workspace files; JSON can still be selected explicitly
for debugging or interchange-friendly inspection. The exact local durability
scope is documented in [Local Durability Target Class](local-durability-target.md).

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
Docker volume `grm-workspaces`. The checked clients create binary workspace
files by default.

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

## Optional grpcurl Smoke Scripts

The branch includes two checked smoke scripts that run `grpcurl` through the
published `fullstorydev/grpcurl` container on the Compose network:

```bash
docker pull fullstorydev/grpcurl:latest

GRPCURL_MODE=docker examples/grpc_demo.sh
GRPCURL_MODE=docker examples/grpc_python_client.py
```

These scripts are intentionally small. They create a workspace, define one
model, create and find one node, and close the workspace. They are not meant to
document the full protobuf JSON surface.

## Security And Durability Notes

This demo uses local autocommit workspace files in the container volume. That is
a tested local workflow, not a hosted durability claim. Treat the container as a
single-writer local service process. `CreateWorkspace` writes an initial
checkpoint, successful supported `ExecuteWorkspace` mutations append durable
operation records, and `OpenWorkspace` replays complete records after the
checkpoint. Binary workspace files are the default; JSON is explicit opt-in.

Before using this shape beyond local development, GRM still needs explicit
service lifecycle, TLS/auth, authorization, limits, audit, recovery, and
coordination design.
