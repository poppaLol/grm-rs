# CLI Session Tutorial

This tutorial walks through a small GRM CLI session from an empty workspace to a
saved and exported graph.

You will:

- define a tiny graph schema
- create nodes and a relationship
- query and traverse the graph
- inspect the logical plan with `session.explain`
- run a first-phase profile with `session.profile`
- save and export the session

## Start A Session

From the repository root:

```bash
cargo run --bin grm -- session
```

You should see:

```text
grm(session)>
```

The session starts empty.

## Use The gRPC Workspace Backend

`cargo run --bin grm -- session` still starts the local embedded CLI session.
When `GRM_BACKEND=grpc` is set, the same entry point opens a service-backed
session instead and routes the supported schema/CRUD/find subset through the
gRPC workspace service. That includes traversal-capable `node.find` for
node/root/end/edge results through `ExecuteWorkspace`. Configure it with
`GRM_SERVICE_ENDPOINT`, `GRM_WORKSPACE_REF`, and optional
`GRM_SERVICE_WORKSPACE_MODE=create|open`; omitted mode means `open`. Service
workspace format defaults to binary; set `GRM_SERVICE_WORKSPACE_FORMAT=json`
only when you explicitly want JSON files. The service-backed CLI prints the
endpoint, workspace ref, create/open mode, persistence format, and
`ExecuteWorkspace` scope before the prompt appears.

The local `--script <path>` startup option is also supported in gRPC mode. The
CLI parses commands locally, sends typed workspace requests, and continues into
the interactive session after the script succeeds. Script text is not sent to
the service as a second command protocol. `--load` remains local-only; use
`GRM_SERVICE_WORKSPACE_MODE=open` to resume a service-managed workspace.

```bash
GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051 \
GRM_WORKSPACE_REF=tutorial-cli \
GRM_SERVICE_WORKSPACE_MODE=create \
cargo run --bin grm -- session
```

Reopen the same service-managed workspace with:

```bash
GRM_BACKEND=grpc \
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051 \
GRM_WORKSPACE_REF=tutorial-cli \
GRM_SERVICE_WORKSPACE_MODE=open \
cargo run --bin grm -- session
```

Typed `session.explain/profile node.find|edge.find` runs through the service
workspace path. Local file commands, transactions, free-form query parity, and
import/export remain local-only or unsupported in service CLI mode. The current
local service path is GRM-owned memory/file backed workspace storage; it is not
Neo4j-backed and does not claim hosted durability, authorization/RBAC,
production certificate lifecycle, or multi-writer coordination.

## Define A Small Graph

Create two node models and one relationship model:

```text
model.define User userId name:string:required
model.define Post postId title:string:required
link.define AUTHORED User Post authoredId year:int:required
```

This says:

- `User` nodes have a required `name`
- `Post` nodes have a required `title`
- `AUTHORED` relationships connect `User` to `Post` and carry a `year`

## Add Data

Create one user, one post, and one relationship:

```text
node.create User name="Alice"
node.create Post title="Graph Notes"
edge.create AUTHORED from=1 to=2 year=2026
```

IDs are backend-assigned. In a fresh in-memory session, the first node is `1`,
the second node is `2`, and the first relationship is `1`.

## Query Nodes And Relationships

Find the user:

```text
node.find User name="Alice"
```

Find relationships from Alice:

```text
edge.find AUTHORED from=1
```

Find Alice's authored posts by traversing the graph:

```text
node.find User name="Alice" via=out:AUTHORED:Post
```

The traversal means:

```text
(User {name: "Alice"}) -[:AUTHORED]-> (Post)
```

## Render Graph-Shaped Output

For traversal queries, you can ask for graph-shaped output:

```text
node.find User name="Alice" via=out:AUTHORED:Post format=graph
```

Example output:

```text
grm(session)> node.find User name="Alice" via=out:AUTHORED:Post format=graph
graph: 2 nodes, 1 links
* (User#1) name=Alice
|
* [AUTHORED#1] year=2026 -> (Post#2) title="Graph Notes"
```

Flat `node.find` and `edge.find` results support text, `jsonl`, and `table`
formats. `format=graph` is for graph-shaped or traversal-shaped results.

## What Gets Indexed Automatically

The current in-memory backend creates and maintains a small set of default
indexes for the graph data it stores:

- node labels
- node properties
- relationship types
- outgoing and incoming adjacency

These are backend-maintained indexes. You do not define them in the CLI today.
They are what make common local lookups and traversals practical, and they are
also why `session.explain` can talk about logical steps such as
`NodePropertySeek`, `RelationshipTypeScan`, and `ExpandOut`.

User-defined indexes are a future direction. For now, model definitions describe
schema and validation; they do not declare custom index policy.

## Explain A Query

`session.explain` shows the current logical plan without running the query:

```text
session.explain node.find User name="Alice" via=out:AUTHORED:Post
```

Example shape:

```text
Current logical plan for node.find User
Plan steps:
  1. NodePropertySeek v0 User.name
  2. ExpandOut v0 -[v1:AUTHORED]-> v2
  3. NodeCheck v2 Post
  4. Return Node v2
```

Verbose explain also shows the simple planner's chosen anchor, candidate access
paths, selected access path, and residual filters. This is conservative
introspection, not a cost-based optimizer.

## Profile A Query

`session.profile` runs the query and reports the plan, result count, and total
elapsed time. Add `--verbose` to include per-step row counts and elapsed time
where the in-memory backend can measure them:

```text
session.profile node.find User name="Alice" via=out:AUTHORED:Post
```

Example shape:

```text
Profile for node.find User
Plan steps:
  1. NodePropertySeek v0 User.name
  2. ExpandOut v0 -[v1:AUTHORED]-> v2
  3. NodeCheck v2 Post
  4. Return Node v2
Result rows: 1
Elapsed: 123us
```

Use `session.explain` when you want to inspect without changing or running the
query. Use `session.profile` when you want to execute and measure the current
query path.

## Save And Reload A Workspace

Save the session:

```text
session.save --json test-dbs/tutorial-session.json
```

Exit and reload it later:

```text
session.exit
```

```bash
cargo run --bin grm -- session --load json test-dbs/tutorial-session.json
```

```text
session.describe
node.find User name="Alice" via=out:AUTHORED:Post
```

Example `session.describe` output:

```text
grm(session)> session.describe
Session Summary
Types defined:
  nodes: Post, User
  links: AUTHORED
Stored rows: 2 nodes, 1 edges
By type:
+------+----------+-------+
| kind | type     | count |
+------+----------+-------+
| node | Post     | 1     |
| node | User     | 1     |
| edge | AUTHORED | 1     |
+------+----------+-------+
Autocommit: off
```

`session.save` / `session.load` are workspace persistence commands. They restore
the local session shape, including runtime schema and graph data. Starting the
CLI with `--load json <path>` or `--load bin <path>` does the same load before
the prompt appears.

If you want successful edits to persist as you work, enable autocommit:

```text
session.autocommit --json test-dbs/tutorial-session.json
```

After that, successful schema changes and data mutations are written to the
autocommit target. Use `session.autocommit status` to inspect it and
`session.autocommit off` to disable it.

Enabling autocommit interactively immediately checkpoints the current in-memory
session to the target path. If that path already contains a session, its current
checkpoint and append log may be replaced. To resume an existing session, load
it explicitly instead:

```text
session.load --json test-dbs/tutorial-session.json
```

For important local data, keep a separate backup as you would for any
application-managed document file.

You can also opt into autocommit when opening an existing session:

```bash
cargo run --bin grm -- session --load json test-dbs/tutorial-session.json --autocommit on
```

Autocommit is a runtime choice, not a setting stored inside the session file.
Without `--autocommit on`, loaded sessions start with autocommit off.

## Export For Interchange

Export a machine-friendly graph document:

```text
session.export --json test-dbs/tutorial-export.json
```

`session.export` is for interchange with other tools and future import/bulk
workflows. It is separate from workspace persistence even though the data may
look similar for small examples.

## Experimental Neo4j Bridge Process

GRM does not yet expose a direct CLI command that pushes an interchange export to
Neo4j. The MCP server can target Neo4j as a live graph backend for the supported
structured create/find tools; the CLI handoff point remains
`session.export --json ...`.

## Script The Setup

You can put setup commands in a `.grm` script:

```text
model.define User userId name:string:required
model.define Post postId title:string:required
link.define AUTHORED User Post authoredId year:int:required
let alice = node.create User name="Alice"
let notes = node.create Post title="Graph Notes"
edge.create AUTHORED from=alice to=notes year=2026
```

The `let` bindings capture the backend-assigned IDs from the `node.create`
commands. Later commands can use those names anywhere a node ID is expected, so
the `AUTHORED` link can connect `alice` to `notes` without knowing their numeric
IDs ahead of time.

Then run:

```bash
cargo run --bin grm -- session --script path/to/setup.grm
```

The script runs first, then the CLI drops into the interactive session with the
scripted graph loaded.

## Where To Go Next

- Use [query language design](../query-language-design.md) for the current query
  grammar and planned query controls.
- Use [import/export](../import-export.md) for interchange format details.
- Use [query and persistence optimization](../perf/query-persistence-optimization.md)
  for explain/profile, planning, and durability direction.
