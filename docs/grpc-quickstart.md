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
defines schema, creates nodes and edges, runs simple finds and a batch request,
closes and reopens the workspace, and verifies data is still present.

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
