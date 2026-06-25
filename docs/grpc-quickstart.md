# GRM gRPC Docker Quick Start

This quick start covers the local gRPC workspace shell. Docker Compose remains
an insecure local demo. The same local server example can also run with TLS from
self-signed or local-CA certificate files for developer smoke tests and the
TLS-capable benchmark line.
The Docker image uses the explicit `docker_local_insecure` profile and should
publish the host port on loopback only.

## Start The Service

Pull and run the published image:

```bash
docker pull lauriebart/grm:latest
docker run --rm --name grm \
  -p 127.0.0.1:50051:50051 \
  -v grm-workspaces:/workspaces \
  lauriebart/grm:latest
```

Alternatively, build the current repository checkout:

```bash
docker compose up --build
```

The service listens on `localhost:50051` and stores local autocommit workspace
files in the `grm-workspaces` Docker volume. Checked service-backed clients use
binary workspace files by default; JSON remains available when a client
explicitly requests `DURABILITY_FORMAT_JSON`.

## Local TLS Service

Generate throwaway localhost certificate material outside the repository. Use a
local CA certificate to sign the server certificate:

```bash
mkdir -p /tmp/grm-tls
openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout /tmp/grm-tls/ca.key \
  -out /tmp/grm-tls/ca.crt \
  -days 1 \
  -subj "/CN=GRM Local CA" \
  -addext basicConstraints=critical,CA:TRUE \
  -addext keyUsage=critical,keyCertSign,cRLSign

openssl req -newkey rsa:2048 -nodes \
  -keyout /tmp/grm-tls/server.key \
  -out /tmp/grm-tls/server.csr \
  -subj /CN=localhost

printf "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n" \
  > /tmp/grm-tls/server.ext

openssl x509 -req \
  -in /tmp/grm-tls/server.csr \
  -CA /tmp/grm-tls/ca.crt \
  -CAkey /tmp/grm-tls/ca.key \
  -CAcreateserial \
  -out /tmp/grm-tls/server.crt \
  -days 1 \
  -sha256 \
  -extfile /tmp/grm-tls/server.ext

openssl req -newkey rsa:2048 -nodes \
  -keyout /tmp/grm-tls/client.key \
  -out /tmp/grm-tls/client.csr \
  -subj /CN=grm-local-client

printf "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n" \
  > /tmp/grm-tls/client.ext

openssl x509 -req \
  -in /tmp/grm-tls/client.csr \
  -CA /tmp/grm-tls/ca.crt \
  -CAkey /tmp/grm-tls/ca.key \
  -CAcreateserial \
  -out /tmp/grm-tls/client.crt \
  -days 1 \
  -sha256 \
  -extfile /tmp/grm-tls/client.ext
```

Start the local workspace server with mutual TLS:

```bash
GRM_SERVICE_TLS_SERVER_CERT=/tmp/grm-tls/server.crt \
GRM_SERVICE_TLS_SERVER_KEY=/tmp/grm-tls/server.key \
GRM_SERVICE_TLS_CLIENT_CA_CERT=/tmp/grm-tls/ca.crt \
cargo run -p grm-service-api --bin grm-local-workspace-server -- \
  127.0.0.1:50051 /tmp/grm-service-workspaces
```

Clients trust the local CA and present their identity with:

```bash
export GRM_SERVICE_TLS_CA_CERT=/tmp/grm-tls/ca.crt
export GRM_SERVICE_TLS_DOMAIN_NAME=localhost
export GRM_SERVICE_TLS_CLIENT_CERT=/tmp/grm-tls/client.crt
export GRM_SERVICE_TLS_CLIENT_KEY=/tmp/grm-tls/client.key
```

Omit the server's `GRM_SERVICE_TLS_CLIENT_CA_CERT` and the client's identity
variables for server-authenticated TLS without client authentication. The
mutual-TLS setup authenticates certificate holders at the transport boundary;
it does not provide RBAC, per-operation authorization, certificate rotation,
hosted durability, or multi-writer coordination.

## Run The Checked Rust Smoke Test

In another shell:

```bash
cargo run -p grm-service-api --example local_workspace_client -- \
  http://127.0.0.1:50051 quickstart-demo
```

For TLS, use an `https://` endpoint and the client trust variables above:

```bash
GRM_SERVICE_TLS_CA_CERT=/tmp/grm-tls/ca.crt \
GRM_SERVICE_TLS_DOMAIN_NAME=localhost \
GRM_SERVICE_TLS_CLIENT_CERT=/tmp/grm-tls/client.crt \
GRM_SERVICE_TLS_CLIENT_KEY=/tmp/grm-tls/client.key \
cargo run -p grm-service-api --example local_workspace_client -- \
  https://127.0.0.1:50051 quickstart-demo
```

This is the primary checked client path. It creates or opens a workspace,
defines schema, creates nodes and edges, runs simple finds, traversal-capable
`node.find` requests for node/root/end/edge results, and a batch request, closes and
reopens the workspace, and verifies data is still present.
Rust callers can use `grm_service_api::GrpcWorkspaceClient` directly for the
same checked subset without building generated protobuf requests manually.

## Seed The Movie Demonstrator Through The CLI

The repository includes a small movie graph at
[`examples/service_movies.grm`](../examples/service_movies.grm). Build the CLI,
then use it to create and populate a service-managed workspace:

```bash
cargo build -p grm-cli

GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051 \
GRM_WORKSPACE_REF=movies-demo \
GRM_SERVICE_WORKSPACE_MODE=create \
target/debug/grm session --script examples/service_movies.grm
```

The CLI parses the script locally and sends each supported command through the
typed `ExecuteWorkspace` API. After setup it remains at the interactive prompt,
with script bindings still available. Some useful queries are:

```text
node.find Person name="Keanu Reeves" via=out:ACTEDIN:Movie order=released:asc
node.find Movie title="The Matrix" via=in:ACTEDIN:Person order=name:asc
edge.find ACTEDIN role="Neo"
session.describe
```

The service console reports sparse operation summaries without model names,
IDs, properties, predicates, or values. A portion of this setup looks like:

```text
workspace_operation completed workspace=movies-demo operation=workspace.create
workspace_operation completed workspace=movies-demo operation=schema.define models_defined=1
workspace_operation completed workspace=movies-demo operation=schema.define links_defined=1
workspace_operation completed workspace=movies-demo operation=node.create nodes_created=1
workspace_operation completed workspace=movies-demo operation=edge.create edges_created=1
workspace_operation completed workspace=movies-demo operation=node.find nodes_read=2
```

Service script execution is fail-fast, but it is not transactional: successful
earlier commands remain committed if a later line fails. Reuse an existing
demonstrator workspace with mode `open`; mode `create` deliberately rejects an
existing workspace ref.

## CLI Service-Backed Workspace Mode

The regular local CLI remains:

```bash
cargo run --bin grm -- session
```

To explicitly route supported CLI session commands through the workspace
service, configure the backend:

```bash
GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=https://127.0.0.1:50051 \
GRM_WORKSPACE_REF=quickstart-cli \
GRM_SERVICE_WORKSPACE_MODE=create \
GRM_SERVICE_TLS_CA_CERT=/tmp/grm-tls/ca.crt \
GRM_SERVICE_TLS_DOMAIN_NAME=localhost \
GRM_SERVICE_TLS_CLIENT_CERT=/tmp/grm-tls/client.crt \
GRM_SERVICE_TLS_CLIENT_KEY=/tmp/grm-tls/client.key \
cargo run --bin grm -- session
```

The CLI prints the selected endpoint, workspace ref, create/open mode,
persistence format, and `ExecuteWorkspace` scope before the prompt appears.
Use `GRM_SERVICE_WORKSPACE_MODE=create` only to initialize a new local
service-managed workspace. Create rejects an existing workspace ref without
altering its checkpoint, append log, or backup; use
`GRM_SERVICE_WORKSPACE_MODE=open` to reopen it:

```bash
GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051 \
GRM_WORKSPACE_REF=quickstart-cli \
GRM_SERVICE_WORKSPACE_MODE=open \
cargo run --bin grm -- session
```

If `GRM_SERVICE_WORKSPACE_MODE` is omitted, the CLI uses `open`.

In this mode, `model.define`, `link.define`, node/edge CRUD, simple find,
traversal-capable `node.find` for node/root/end/edge results, typed
`session.explain/profile node.find|edge.find`, `model.list`, `link.list`, and
`session.describe` use `ExecuteWorkspace`.
The CLI also supports `--script <path>` by parsing the file locally and routing
the supported commands through `ExecuteWorkspace`. Local session file loading,
transactions, free-form query parity, and import/export remain local-only or
unsupported in service CLI mode.
`GRM_SERVICE_WORKSPACE_FORMAT` defaults to binary; set it to `json` only when
you explicitly want JSON workspace files. The local Docker service stores these
workspace files under its configured workspace root; this is checked local
single-writer persistence behavior, not a hosted durability, RBAC/authorization,
production certificate-lifecycle, or multi-writer guarantee. See
[Local Durability Target Class](local-durability-target.md) for the exact
create/open/reopen, checkpoint, autocommit, and unsupported-case scope.

## Python Service Mode

The Python package keeps the embedded `Session` API. It also exposes
`ServiceSession` for the checked service subset:

```python
from grm_rs import ServiceSession

session = ServiceSession(
    endpoint="https://127.0.0.1:50051",
    workspace_ref="quickstart-python",
    mode="create",
    tls_ca_cert="/tmp/grm-tls/ca.crt",
    tls_domain_name="localhost",
    tls_client_cert="/tmp/grm-tls/client.crt",
    tls_client_key="/tmp/grm-tls/client.key",
)
session.model_create("User", "userId", [{"name": "name", "type": "string", "required": True}])
session.node_create("User", {"name": "Ada"})
assert len(session.node_find("User", {"name": "Ada"})) == 1
```

`ServiceSession(..., workspace_format="binary")` is the default. Use
`workspace_format="json"` explicitly for JSON workspace files.

MCP service mode uses the same client trust variables:

```bash
GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=https://127.0.0.1:50051 \
GRM_WORKSPACE_REF=quickstart-mcp \
GRM_SERVICE_WORKSPACE_MODE=create \
GRM_SERVICE_TLS_CA_CERT=/tmp/grm-tls/ca.crt \
GRM_SERVICE_TLS_DOMAIN_NAME=localhost \
GRM_SERVICE_TLS_CLIENT_CERT=/tmp/grm-tls/client.crt \
GRM_SERVICE_TLS_CLIENT_KEY=/tmp/grm-tls/client.key \
cargo run -p grm-mcp
```

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

Stop the published-image container with:

```bash
docker stop grm
```

For the Compose service, use:

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
- The local server example supports TLS for local certificate material through
  `GRM_SERVICE_TLS_SERVER_CERT` and `GRM_SERVICE_TLS_SERVER_KEY`, and mutual TLS
  through `GRM_SERVICE_TLS_CLIENT_CA_CERT`.
- The current service-backed durability target is single-writer local filesystem
  behavior with binary workspace files by default; JSON is explicit opt-in.
