# grm-rs Python bindings

This crate packages a first-pass Python API for the `grm-rs` runtime session surface.
It includes embedded `Session`, service-backed `ServiceSession`, and direct
`Neo4jSession` implementations of a portable synchronous `GraphSession` core.

The Python package is an alpha pre-release. Its API and supported backend
capabilities may change between alpha versions, but it is published for public
evaluation, tutorials, and early application development.

Install the latest published alpha from PyPI:

```bash
python -m pip install --pre grm-rs
```

Pin a specific release when reproducibility matters:

```bash
python -m pip install grm-rs==0.1.0a7
```

The distribution package is named `grm-rs`; the import package is `grm_rs`.

## Build

```bash
cd grm-python
maturin develop
```

Build a shareable wheel from the repository root:

```bash
python -m pip install maturin
maturin build --manifest-path grm-python/Cargo.toml --release --out dist
```

See [`../docs/python-package-distribution.md`](../docs/python-package-distribution.md)
for local builds and release details.

## Example

```python
from grm_rs import GraphId, GraphSession, Session


def add_user(session: GraphSession, name: str) -> GraphId:
    return session.node_create("User", {"name": name})["id"]

session = Session()
session.model_create(
    "User",
    "userId",
    [
        {"name": "name", "type": "string", "required": True},
        {"name": "age", "type": "int", "required": False},
    ],
)
session.model_create(
    "Post",
    "postId",
    [
        {"name": "title", "type": "string", "required": True},
    ],
)
session.link_create(
    "AUTHORED",
    "User",
    "Post",
    "authoredId",
    [
        {"name": "year", "type": "int", "required": True},
    ],
)

alice = session.node_create("User", {"name": "Alice", "age": 42})
post = session.node_create("Post", {"title": "Hello"})
edge = session.edge_create("AUTHORED", alice["id"], post["id"], {"year": 2024})
users = session.node_find("User", {"name": "Alice"})
```

The same `add_user` function accepts `Session`, `ServiceSession`, or
`Neo4jSession`; applications do not need to redeclare the session API.

## Portable Store Injection

Application stores can depend on `GraphSession` and receive either backend:

```python
from grm_rs import GraphSession, Neo4jSession, ServiceSession


class UserStore:
    def __init__(self, graph: GraphSession) -> None:
        self.graph = graph

    def find_named(self, name: str):
        return self.graph.node_find("User", {"name": name})


graph: GraphSession
if use_neo4j:
    graph = Neo4jSession(uri=uri, user=user, password=password)
else:
    graph = ServiceSession(endpoint=endpoint, workspace_ref="demo", mode="open")

users = UserStore(graph).find_named("Ada")
```

The portable core has compatible schema inspection, CRUD/find, and atomic
batch semantics. `GraphId` is opaque application-facing identity: code may pass
returned IDs back to the same graph session, but must not assume physical ID
preservation when moving data between backends.

| Capability | Portable `GraphSession` | Workspace only | Neo4j native |
| --- | --- | --- | --- |
| Schema define/list | yes | yes | yes, GRM-owned session metadata |
| Node/edge CRUD and simple find | yes | yes | yes |
| Atomic batch writes | yes | yes | yes, one Neo4j transaction |
| Non-atomic batch writes | no | yes | no |
| Traversal-shaped `node_find` | no | yes | no |
| Explain/profile, indexes, import/export | no | yes | no |
| Native Cypher via `execute_query` | no | no | yes |

Use `session.capabilities()` for lightweight discovery. The stable capability
names currently include `graph`, `atomic_batch`, `non_atomic_batch`,
`workspace`, `traversal`, `explain_profile`, `persistence`, and
`neo4j_native_query`; applications should test membership rather than assume
every session has optional methods.

Traversal queries mirror CLI `node.find ... via=...` semantics with structured
Python inputs:

```python
posts = session.node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
)

authored_edges = session.node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
    end_filters={"title": "Hello"},
    edge_filters={"year": 2024},
    return_="edge",
)
```

Explain and profile use the same query arguments and return structured plan
data:

```python
plan = session.explain_node_find(
    "User",
    {"name": "Alice"},
    via=[{"dir": "out", "link": "AUTHORED", "model": "Post"}],
)

profile = session.profile_edge_find("AUTHORED", {"from": alice["id"]})
assert profile["result_rows"] >= 0

indexes = session.indexes()
assert indexes["indexes"][0]["kind"] == "system"
```

Index catalog entries describe GRM-maintained derived acceleration structures
such as node-label, exact node-property, relationship-type, and adjacency
indexes. They are not user-defined indexes, and their contents are not durable
source-of-truth data.

Batch operations share the same runtime semantics as MCP `grm_batch`, including
atomic rollback, indexed errors, batch-local refs, and one autocommit snapshot
after successful applied operations:

```python
result = session.batch(
    [
        {"op": "node_create", "args": {"model": "User", "props": {"name": "Bob"}, "ref": "bob"}},
        {"op": "node_create", "args": {"model": "Post", "props": {"title": "Hello"}, "ref": "post"}},
        {
            "op": "edge_create",
            "args": {"model": "AUTHORED", "from": "bob", "to": "post", "props": {"year": 2026}},
        },
    ],
    response="detailed",
)
```

The portable `GraphSession.batch()` contract is atomic-only and therefore has
no `atomic` argument. For Neo4j it stages GRM schema changes, executes all graph
writes in one backend transaction, and installs staged schema only after
commit. A failed operation rolls back graph writes and discards staged schema.
Workspace sessions additionally implement `WorkspaceBatchCapability`, whose
`atomic=False` mode is intentionally outside the portable contract.

Use `save_json` / `load_json` or `save_binary` / `load_binary` for local
workspace snapshots. Use `export_json`, `export_dict`, and `import_json` for
portable `grm.interchange` graph files; `import_json` currently imports into an
empty session only.

## Service-Backed Session

`ServiceSession` routes supported operations through the gRPC workspace service
using workspace-scoped `ExecuteWorkspace`. The supported subset is schema
define/list, node and edge create/update/delete/find, traversal-capable
`node_find` for node/root/end/edge results, batch, and reopen by constructing another
`ServiceSession` with `mode="open"` and the same `workspace_ref`.

```python
from grm_rs import ServiceSession

session = ServiceSession(
    endpoint="http://127.0.0.1:50051",
    workspace_ref="python-demo",
    mode="create",
)
session.model_create("User", "userId", [{"name": "name", "type": "string", "required": True}])
ada = session.node_create("User", {"name": "Ada"})
assert session.node_find("User", {"id": ada["id"]})[0]["props"]["name"] == "Ada"

reopened = ServiceSession(
    endpoint="http://127.0.0.1:50051",
    workspace_ref="python-demo",
    mode="open",
)
assert len(reopened.node_find("User", {"name": "Ada"})) == 1
```

For a local TLS service, use an `https://` endpoint and either pass trust
material explicitly:

```python
session = ServiceSession(
    endpoint="https://127.0.0.1:50051",
    workspace_ref="python-demo",
    mode="create",
    tls_ca_cert="/tmp/grm-tls/ca.crt",
    tls_domain_name="localhost",
    tls_client_cert="/tmp/grm-tls/client.crt",
    tls_client_key="/tmp/grm-tls/client.key",
)
```

or set `GRM_SERVICE_TLS_CA_CERT`, `GRM_SERVICE_TLS_DOMAIN_NAME`,
`GRM_SERVICE_TLS_CLIENT_CERT`, and `GRM_SERVICE_TLS_CLIENT_KEY` in the
environment before constructing `ServiceSession`. Client certificate/key
parameters are paired and are required when the server enforces mutual TLS.

Service workspaces use binary persistence by default. Pass
`workspace_format="json"` explicitly when you need JSON workspace files. The
Python service path supports typed traversal-capable `node_find`,
`explain_node_find`, and `profile_node_find` through workspace-scoped service
requests. Direct unscoped service RPCs, free-form query parity, import/export,
hosted durability, RBAC, production certificate lifecycle, and multi-writer
coordination are not provided by this Python service path.
