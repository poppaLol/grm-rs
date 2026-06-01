# grm-rs Python bindings

This crate packages a first-pass Python API for the `grm-rs` runtime session surface.
It includes the embedded/local `Session` API and a service-backed
`ServiceSession` API for the checked gRPC workspace subset.

The current Python package is a pre-release. It is meant for private wheel
sharing and GitHub Release testing before any public PyPI upload.

Install from a wheel file:

```bash
python -m pip install ./dist/grm_rs-0.1.0a5-*.whl
```

Or install from an authenticated GitHub Release asset URL:

```bash
python -m pip install "https://github.com/<owner>/<repo>/releases/download/grm-python-v0.1.0a5/<wheel-file>.whl"
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
for private sharing and pre-release options.

## Example

```python
from grm_rs import Session

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

Use `save_json` / `load_json` or `save_binary` / `load_binary` for local
workspace snapshots. Use `export_json`, `export_dict`, and `import_json` for
portable `grm.interchange` graph files; `import_json` currently imports into an
empty session only.

## Service-Backed Session

`ServiceSession` routes supported operations through the gRPC workspace service
using workspace-scoped `ExecuteWorkspace`. The supported subset is schema
define/list, node and edge create/update/delete/find, traversal-capable
`node_find` for node/root/end results, batch, and reopen by constructing another
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

Service workspaces use binary persistence by default. Pass
`workspace_format="json"` explicitly when you need JSON workspace files. Direct
unscoped service RPCs, `node_find(return_="edge")`, free-form query parity,
explain/profile, import/export, hosted durability, auth/TLS, and multi-writer
coordination are not provided by this Python service path.
