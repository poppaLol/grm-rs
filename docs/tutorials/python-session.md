# Python Session Tutorial

This tutorial mirrors the [CLI session tutorial](cli-session.md), but uses the
`grm_rs.Session` Python API for the same workflow.

Python methods are adapter conveniences over GRM's shared typed runtime
operations. In particular, structured `node_find(...)` traversal and
`explain_node_find(...)` / `profile_node_find(...)` calls are represented inside
the runtime as typed request objects rather than CLI command strings.

You will:

- define a tiny graph schema
- create nodes and a relationship
- query and traverse the graph
- inspect the logical plan with `explain_node_find`
- run a first-phase profile with `profile_node_find`
- save and export the session

## Install The Extension

From the repository root:

```bash
python -m venv .venv
source .venv/bin/activate
pip install maturin
cd grm-python
mkdir -p test-dbs
maturin develop
```

`maturin develop` compiles the Rust extension and installs it into the active
virtualenv, so `import grm_rs` works immediately in that environment.

## Start A Session

Create a file named `tutorial_session.py`:

```python
from grm_rs import Session

session = Session()
```

The session starts empty.

## Define A Small Graph

Create two node models and one relationship model:

```python
session.model_create(
    "User",
    "userId",
    [
        {"name": "name", "type": "string", "required": True},
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
```

This says:

- `User` nodes have a required `name`
- `Post` nodes have a required `title`
- `AUTHORED` relationships connect `User` to `Post` and carry a `year`

## Add Data

Create one user, one post, and one relationship:

```python
alice = session.node_create("User", {"name": "Alice"})
notes = session.node_create("Post", {"title": "Graph Notes"})
authored = session.edge_create(
    "AUTHORED",
    alice["id"],
    notes["id"],
    {"year": 2026},
)
```

IDs are backend-assigned. The returned dictionaries include the assigned `id`,
so Python code usually keeps the returned node objects instead of assuming
specific numeric IDs.

## Query Nodes And Relationships

Find the user:

```python
users = session.node_find("User", {"name": "Alice"})
print(users)
```

Find relationships from Alice:

```python
edges = session.edge_find("AUTHORED", {"from": alice["id"]})
print(edges)
```

Find Alice's authored posts by traversing the graph:

```python
posts = session.node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
)
print(posts)
```

The traversal means:

```text
(User {name: "Alice"}) -[:AUTHORED]-> (Post)
```

Python traversal mirrors CLI `node.find ... via=...` semantics, but uses
structured inputs. The `via` list contains one dictionary per traversal step.

## Return Relationship Results

Traversal queries return end nodes by default. To return the traversed
relationship instead, pass `return_="edge"`:

```python
authored_edges = session.node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
    end_filters={"title": "Graph Notes"},
    edge_filters={"year": 2026},
    return_="edge",
)
print(authored_edges)
```

Use `end_filters` for the node reached by the traversal and `edge_filters` for
properties on the relationship.

## What Gets Indexed Automatically

The current in-memory backend creates and maintains a small set of default
indexes for the graph data it stores:

- node labels
- node properties
- relationship types
- outgoing and incoming adjacency

These are backend-maintained indexes. You do not define them from Python today.
They are what make common local lookups and traversals practical, and they are
also why `explain_node_find` can talk about logical steps such as
`NodePropertySeek`, `RelationshipTypeScan`, and `ExpandOut`.

You can inspect the current index catalog:

```python
print(session.indexes())
```

User-defined indexes are a future direction. For now, model definitions describe
schema and validation; they do not declare custom index policy.

## Explain A Query

`explain_node_find` shows the current logical plan without running the query:

```python
plan = session.explain_node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
)
print(plan["plan"]["text"])
```

The exact rendered text is allowed to evolve, but the plan shape will look like
these steps:

```text
1. NodePropertySeek v0 User.name
2. ExpandOut v0 -[v1:AUTHORED]-> v2
3. NodeCheck v2 Post
4. Return Node v2
```

The returned value is a Python dictionary, so you can also inspect
`plan["plan"]["steps"]` or `plan["plan"]["details"]`.

## Profile A Query

`profile_node_find` runs the query and reports the plan, result count, and total
elapsed time:

```python
profile = session.profile_node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
)
print(profile["result_rows"], profile["elapsed"]["display"])
print(profile["plan"]["text"])
```

Use `explain_node_find` when you want to inspect without changing or running the
query. Use `profile_node_find` when you want to execute and measure the current
query path.

## Save And Reload A Workspace

Save the session:

```python
session.save_json("test-dbs/tutorial-session.json")
```

Reload it later:

```python
reloaded = Session()
reloaded.load_json("test-dbs/tutorial-session.json")

print(reloaded.model_list())
print(
    reloaded.node_find(
        "User",
        {"name": "Alice"},
        via=[
            {"dir": "out", "link": "AUTHORED", "model": "Post"},
        ],
    )
)
```

`save_json` / `load_json` are workspace persistence methods. They restore the
local session shape, including runtime schema and graph data. Binary persistence
is also available with `save_binary` and `load_binary`.

If you want successful edits to persist as you work, enable autocommit when you
create the session:

```python
session = Session(
    autocommit=True,
    autocommit_path="test-dbs/tutorial-session.json",
)
```

When autocommit is enabled, successful schema changes and data mutations are
persisted through the shared append-log/checkpoint path. Autocommit is a
runtime choice, not a setting stored inside the session file.

## Export For Interchange

Export a machine-friendly graph document:

```python
session.export_json("test-dbs/tutorial-export.json")
```

You can also get the same document as a Python dictionary:

```python
portable = session.export_dict()
print(portable["format"], portable["version"])
```

`export_json` and `export_dict` are for interchange with other tools and future
bulk workflows. They are separate from workspace persistence even though the
data may look similar for small examples.

## Import Interchange

Import reads the same interchange document into a fresh session:

```python
fresh = Session()
fresh.import_json("test-dbs/tutorial-export.json")
print(fresh.node_find("User", {"name": "Alice"}))
```

Import currently requires an empty session. If the session already has schema or
graph data, GRM raises `grm_rs.GrmError` instead of merging or replacing
contents.

## Batch The Setup

You can apply the same setup as one structured batch:

```python
session = Session()

result = session.batch(
    [
        {
            "op": "schema_define_node",
            "args": {
                "name": "User",
                "id_field": "userId",
                "fields": [
                    {"name": "name", "type": "string", "required": True},
                ],
            },
        },
        {
            "op": "schema_define_node",
            "args": {
                "name": "Post",
                "id_field": "postId",
                "fields": [
                    {"name": "title", "type": "string", "required": True},
                ],
            },
        },
        {
            "op": "schema_define_edge",
            "args": {
                "name": "AUTHORED",
                "from_model": "User",
                "to_model": "Post",
                "id_field": "authoredId",
                "fields": [
                    {"name": "year", "type": "int", "required": True},
                ],
            },
        },
        {
            "op": "node_create",
            "args": {
                "model": "User",
                "props": {"name": "Alice"},
                "ref": "alice",
            },
        },
        {
            "op": "node_create",
            "args": {
                "model": "Post",
                "props": {"title": "Graph Notes"},
                "ref": "notes",
            },
        },
        {
            "op": "edge_create",
            "args": {
                "model": "AUTHORED",
                "from": "alice",
                "to": "notes",
                "props": {"year": 2026},
            },
        },
    ],
    atomic=True,
    response="detailed",
)

print(result["applied"])
print(result["counts"])
```

The batch-local `ref` values capture node IDs from earlier operations in the
same batch. Later operations can use those names anywhere a node ID is expected,
so the `AUTHORED` link can connect `alice` to `notes` without knowing their
numeric IDs ahead of time.

## Complete Example

Here is the whole tutorial as one script:

```python
from grm_rs import Session


session = Session()

session.model_create(
    "User",
    "userId",
    [
        {"name": "name", "type": "string", "required": True},
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

alice = session.node_create("User", {"name": "Alice"})
notes = session.node_create("Post", {"title": "Graph Notes"})
session.edge_create("AUTHORED", alice["id"], notes["id"], {"year": 2026})

print(session.node_find("User", {"name": "Alice"}))
print(session.edge_find("AUTHORED", {"from": alice["id"]}))
print(
    session.node_find(
        "User",
        {"name": "Alice"},
        via=[
            {"dir": "out", "link": "AUTHORED", "model": "Post"},
        ],
    )
)

plan = session.explain_node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
)
print(plan["plan"]["text"])

profile = session.profile_node_find(
    "User",
    {"name": "Alice"},
    via=[
        {"dir": "out", "link": "AUTHORED", "model": "Post"},
    ],
)
print(profile["result_rows"], profile["elapsed"]["display"])

session.save_json("test-dbs/tutorial-session.json")
session.export_json("test-dbs/tutorial-export.json")
```

## Where To Go Next

- Use [Python quickstart](../python-quickstart.md) for a compact Python API
  overview.
- Use [import/export](../import-export.md) for interchange format details.
- Use [Python Neo4j API expansion](../python-neo4j-api-expansion.md) for the
  live Neo4j backend direction.
