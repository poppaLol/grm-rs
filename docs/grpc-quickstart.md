# GRM gRPC Docker Quick Start

This quick start covers the local Docker-hosted gRPC workspace shell that has
been smoke-tested in this branch. It is an insecure local demo, not a production
daemon.

## Start The Service

```bash
docker compose up --build
```

The service listens on `localhost:50051` and stores local autocommit workspace
files in the `grm-workspaces` Docker volume. Checked service-backed clients use
binary workspace files by default; JSON remains available when a client
explicitly requests `DURABILITY_FORMAT_JSON`.

## Run The Checked Rust Smoke Test

In another shell:

```bash
cargo run -p grm-service-api --example local_workspace_client -- \
  http://127.0.0.1:50051 quickstart-demo
```

This is the primary checked client path. It creates or opens a workspace,
defines schema, creates nodes and edges, runs simple finds, traversal-capable
`node.find` requests for node/root/end/edge results, and a batch request, closes and
reopens the workspace, and verifies data is still present.
Rust callers can use `grm_service_api::GrpcWorkspaceClient` directly for the
same checked subset without building generated protobuf requests manually.

## CLI Service Mode

The regular local CLI remains:

```bash
cargo run --bin grm -- session
```

To explicitly route supported CLI session commands through the workspace
service, configure the backend:

```bash
GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051 \
GRM_WORKSPACE_REF=quickstart-cli \
GRM_SERVICE_WORKSPACE_MODE=create \
cargo run --bin grm -- session
```

In this mode, `model.define`, `link.define`, node/edge CRUD, simple find,
traversal-capable `node.find` for node/root/end/edge results, `model.list`,
`link.list`, and `session.describe` use `ExecuteWorkspace`.
Local session file commands, transactions, explain/profile, free-form query
parity, and import/export remain local-only or
unsupported in service CLI mode.
`GRM_SERVICE_WORKSPACE_FORMAT` defaults to binary; set it to `json` only when
you explicitly want JSON workspace files.

## Python Service Mode

The Python package keeps the embedded `Session` API. It also exposes
`ServiceSession` for the checked service subset:

```python
from grm_rs import ServiceSession

session = ServiceSession(
    endpoint="http://127.0.0.1:50051",
    workspace_ref="quickstart-python",
    mode="create",
)
session.model_create("User", "userId", [{"name": "name", "type": "string", "required": True}])
session.node_create("User", {"name": "Ada"})
assert len(session.node_find("User", {"name": "Ada"})) == 1
```

`ServiceSession(..., workspace_format="binary")` is the default. Use
`workspace_format="json"` explicitly for JSON workspace files.

## Optional grpcurl Smoke Scripts

The repo also includes two small smoke scripts that use the published
`fullstorydev/grpcurl` container on the Compose network. These were checked
against the Docker service during this branch:

```bash
docker pull fullstorydev/grpcurl:latest

GRPCURL_MODE=docker examples/grpc_demo.sh
GRPCURL_MODE=docker examples/grpc_python_client.py
```

These examples exercise a minimal workspace flow: create workspace, define a
`User` model, create one node, find it, and close the workspace. They are useful
for checking protobuf JSON request shape, but the Rust client remains the more
complete demo.

## Stop The Service

```bash
docker compose down
```

Remove the demo workspace volume when you want a clean slate:

```bash
docker compose down -v
```

## Notes

- The server does not expose gRPC reflection.
- Direct non-workspace RPC families are intentionally unsupported by the local
  shell.
- The Docker demo does not provide TLS, authentication, hosted durability, or
  multi-writer coordination.
