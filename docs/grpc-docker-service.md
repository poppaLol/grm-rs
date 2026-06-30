# GRM gRPC Docker Service

This is an insecure Docker-hostable demo for the local GRM gRPC workspace shell.
It is intended for local development, examples, and adapter integration tests.
It is not the TLS-capable benchmark line and does not provide authentication,
authorization, hosted durability, or multi-writer coordination.
The image opts into the explicit `docker_local_insecure` service profile so the
process can bind inside the container while Docker publishes the host port on
loopback only.

The container runs the `grm-service-api` `grm-local-workspace-server` binary:

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

Current non-goals for this insecure Compose profile:

- production daemon lifecycle
- TLS/mTLS or authentication in `docker-compose.yml`
- authorization, quotas, request limits, and audit
- hosted durability or multi-writer guarantees
- direct RPC-family parity outside `ExecuteWorkspace`
- full traversal/query/explain/profile/import/export parity through all adapters

A separate local controlled secured demo lives in
[`docs/security/cfssl-mtls-local-demo.md`](security/cfssl-mtls-local-demo.md)
and `docker-compose.cfssl-mtls.yml`. It uses CFSSL-generated local demo
certificates, explicit certificate fingerprint mapping, and the versioned
permission table. It does not turn this insecure profile into a secured one.

## Run The Published Image

The ready-built service is published as `lauriebart/grm:latest`:

```bash
docker pull lauriebart/grm:latest
docker run --rm --name grm \
  -p 127.0.0.1:50051:50051 \
  -v grm-workspaces:/workspaces \
  lauriebart/grm:latest
```

The service listens on `localhost:50051`. Workspace data persists in the named
`grm-workspaces` volume when the container stops.

Stop the foreground container from another shell with:

```bash
docker stop grm
```

## Automated Docker Hub Publishing

GitHub Actions publishes `lauriebart/grm` through
`.github/workflows/docker-publish.yml`. Configure these repository settings:

- Actions variable `DOCKERHUB_USERNAME`: `lauriebart`
- Actions secret `DOCKERHUB_TOKEN`: a Docker Hub access token with permission
  to push `lauriebart/grm`

The workflow builds and smoke-runs the container before logging in and pushing.
A push to `main` updates `latest` and publishes an immutable `sha-<commit>` tag.
A Git tag such as `grm-v0.1.0` publishes `0.1.0` and the immutable SHA tag.
The workflow can also be started manually from GitHub Actions.

Use a Docker Hub access token rather than storing the account password. The
token is a repository secret and must never be committed to this repository or
printed by workflow steps.

## Build With Docker Compose

Contributors can build the service from the current checkout:

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

## TLS-Capable Local Service

The `grm-local-workspace-server` binary can run with local TLS certificate files
outside the default insecure Docker Compose demo:

```bash
GRM_SERVICE_TLS_SERVER_CERT=/tmp/grm-tls/server.crt \
GRM_SERVICE_TLS_SERVER_KEY=/tmp/grm-tls/server.key \
GRM_SERVICE_TLS_CLIENT_CA_CERT=/tmp/grm-tls/ca.crt \
cargo run -p grm-service-api --bin grm-local-workspace-server -- \
  127.0.0.1:50051 /tmp/grm-service-workspaces
```

Rust, CLI, Python, and MCP clients trust the local CA certificate with
`GRM_SERVICE_TLS_CA_CERT=/tmp/grm-tls/ca.crt` and
`GRM_SERVICE_TLS_DOMAIN_NAME=localhost`, and authenticate with
`GRM_SERVICE_TLS_CLIENT_CERT` plus `GRM_SERVICE_TLS_CLIENT_KEY`. Python can pass
the corresponding `tls_ca_cert=`, `tls_domain_name=`, `tls_client_cert=`, and
`tls_client_key=` parameters to `ServiceSession`. This proves local
certificate-based transport authentication only; it is not RBAC, production
certificate lifecycle, hosted durability, or multi-writer coordination.

For a repeatable local mTLS onboarding path that also wires certificate
fingerprint mappings and permission-table authorization, use the CFSSL demo:

```bash
examples/cfssl-mtls/scripts/run-compose-smoke.sh
```

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

The service writes one aggregate completion line for each successful workspace
operation. Logs include the workspace ref or opaque handle, operation family,
and separate aggregate counts for node models, link models, node records, and
edge records. Zero-valued counts are omitted. Logs do not include model names,
record IDs, properties, predicates, query text, or returned values.

```text
workspace_operation completed workspace=demo operation=node.create nodes_created=1
```

Before using this shape beyond local development, GRM still needs explicit
service lifecycle, auth, authorization, limits, audit, recovery, production
certificate lifecycle, and coordination design.
