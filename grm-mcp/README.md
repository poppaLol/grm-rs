# grm-mcp

`grm-mcp` exposes a `grm-rs` runtime graph session as a local Model Context
Protocol server.

## Build

```bash
cargo build -p grm-mcp
```

## Client Configuration

For an MCP client such as Ollmcp, run the compiled binary over stdio:

```json
{
  "mcpServers": {
    "grm": {
      "command": "/workspaces/grm-rs/target/debug/grm-mcp",
      "args": [
        "--import-json",
        "/path/to/graph.export.json"
      ]
    }
  }
}
```

The server owns one in-memory `SessionState` per process. Use
`--autocommit-json <path>` or `--autocommit-bin <path>` if you want tool
mutations persisted after each write. Use `--export-json <path>` if you also
want a readable interchange graph file updated after successful mutations.

### Neo4j MCP Mode

To let agents write directly into a live Neo4j graph, run `grm-mcp` with:

```bash
GRM_BACKEND=neo4j
GRM_SCHEMA_TEMPLATE=project-memory-schema.json
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=...
grm-mcp
```

`GRM_SCHEMA_TEMPLATE` is optional. When set, `grm-mcp` treats the path as a
local GRM JSON session file for Neo4j runtime schema memory. If the file exists,
the server recovers the session-local runtime schema from it. If the file is
missing, the server starts with an empty schema and creates a fresh local file.
Schema definitions made through `grm_schema_define_node`,
`grm_schema_define_edge`, or Neo4j-supported `grm_batch` schema ops are appended
to that local file as they are built. This does not create Neo4j nodes or
relationships, and it does not persist schema metadata into Neo4j.

Neo4j mode is intentionally narrow. It supports:

- `grm_schema_list`
- `grm_batch` for `schema_define_node`, `schema_define_edge`, `node_create`,
  `node_update`, `node_delete`, `edge_create`, `edge_update`, and `edge_delete`
- `grm_schema_define_node`
- `grm_schema_define_edge`
- `grm_node_create`
- `grm_node_update`
- `grm_node_delete`
- `grm_edge_create`
- `grm_edge_update`
- `grm_edge_delete`
- simple `grm_node_find`
- simple `grm_edge_find`

Important: runtime schema metadata is session-local in this first slice. If you
restart `grm-mcp` without `GRM_SCHEMA_TEMPLATE`, the Neo4j graph data remains,
but agents must define or reconstruct the runtime schema again before finding or
extending typed data. `grm_schema_list` is still the first schema inspection
tool. Agents should also inspect `grm://backend/status`, which reports the
backend mode, runtime schema model count, whether the runtime schema is empty,
whether schema memory persistence is enabled, and whether schema memory was
recovered from an existing file. If the schema is empty, ask whether to define a
fresh schema or reconstruct one from project docs before writing. If an existing
local schema memory file is invalid or inconsistent, startup fails loudly.

Agent/tool flow after startup:

1. Call `grm_schema_list`.
2. Read `grm://backend/status`.
3. If `schema_template_loaded` is `true`, the server recovered schema memory
   from the local file; treat the models returned by `grm_schema_list` as the
   current runtime schema and verify the intended write matches those fields and
   endpoints.
4. If `runtime_schema_empty` is `true`, define schema with
   `grm_schema_define_node`, `grm_schema_define_edge`, or a `grm_batch`
   containing `schema_define_node`/`schema_define_edge` ops. If
   `schema_template_persistence_enabled` is `true`, those schema definitions are
   persisted to the configured local file.
5. Only then write graph data with `grm_batch`, `grm_node_create`,
   `grm_node_update`, `grm_node_delete`, `grm_edge_create`, `grm_edge_update`,
   or `grm_edge_delete`.

For autonomous schema-design tasks, grant that permission in the task prompt
rather than relying on the conservative built-in help text. For example:

```text
You may design and define the GRM runtime schema for this Neo4j memory task.
First call grm_schema_list and inspect grm://backend/status. If the runtime
schema is empty or missing required models, choose a compact schema, define it
with grm_batch schema_define_node/schema_define_edge operations, then create the
requested graph data. Do not infer schema from Neo4j labels/properties, and do
not write anything until the runtime schema contains the target models.
```

Graph durability comes from Neo4j, not the GRM WAL/autocommit layer. Neo4j mode
does not support snapshots, import/export, autocommit, explain/profile,
or traversal/query parity yet. Unsupported tools and unsupported Neo4j batch
operations return clear not-supported errors.

## Startup Flags

```text
--load-json <path>
--load-bin <path>
--import-json <path>
--export-json <path>
--autocommit-json <path>
--autocommit-bin <path>
```

## Example Tool Calls

Agents should start with `grm_help`, then inspect `grm://schema` or call
`grm_schema_list` before graph operations. If a GRM tool fails in a way the
agent cannot immediately fix, call `grm_tool_help` with the failed tool name
before retrying.

Get server guidance:

```json
{}
```

Get operation-specific recovery help:

```json
{
  "tool": "grm_node_create"
}
```

Find a user:

```json
{
  "model": "User",
  "filters": {
    "name": "Alice Jones"
  }
}
```

Run a CLI-compatible traversal query:

```json
{
  "command": "node.find User name=\"Alice Jones\" via=out:AUTHORED:Post"
}
```

Call `grm_explain` or `grm_profile` with the same command to get structured
plan data, row counts, and elapsed time:

```json
{
  "command": "node.find User name=\"Alice Jones\" via=out:AUTHORED:Post"
}
```

Export the current graph without writing a file:

```json
{
  "path": null
}
```

## Resources

```text
grm://docs/agent-guide
grm://docs/tool-help
grm://schema
grm://graph/export
grm://graph/summary
grm://docs/query-language
```

## Bulk Writes

Agents often create extracted graphs one entity at a time when only
single-operation tools are visible. Use `grm_batch` when applying more than a
few ordered schema, node, or edge mutations:

```json
{
  "atomic": true,
  "allow_deletes": false,
  "response": "summary",
  "ops": [
    {
      "op": "node_create",
      "args": {
        "ref": "user:alice",
        "model": "User",
        "props": {
          "name": "Alice Jones"
        }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "post:hello",
        "model": "Post",
        "props": {
          "title": "Hello World"
        }
      }
    },
    {
      "op": "edge_create",
      "args": {
        "model": "AUTHORED",
        "from": "user:alice",
        "to": "post:hello",
        "props": {}
      }
    }
  ]
}
```

`grm_batch` applies operations in order. By default, batches are atomic and
return a compact summary grouped by operation and model. Use
`"response": "detailed"` when you need created or updated ids back. Node create
operations may provide a batch-local `ref`, and later edge create operations may
use either numeric ids or those earlier refs as endpoints. Refs must be unique
within a batch. Delete operations are rejected unless `allow_deletes` is set to
`true`.

In Neo4j mode, `grm_batch` currently requires `atomic=true` and supports
schema definition plus single node/edge create, update, and delete operations.
It stages session-local schema metadata and executes Neo4j graph mutations in one
transaction, committing only after every supported operation succeeds. It does
not auto-create schema from data writes; creating or finding a model that is not
registered in the session-local runtime schema fails with guidance to define
schema first.

`grm_graph_patch` remains the planned declarative graph-shaped bulk write
surface.

## Modeling Guidance

Before defining schema, decide the graph's richness vs sparseness.

Use richer, more specific node and edge models when concepts have distinct
fields, constraints, relationship patterns, or query meaning. For example,
separate `Knife`, `Plate`, and `Fork` node models can make sense when each has
different fields or participates in different relationships. Separate `AUTHORED`,
`PURCHASED`, `LOCATEDIN`, and `DEPENDSON` edge models make sense when the
relationship semantics drive different traversals or properties.

Use sparser, broader node and edge models when instances share one shape and
differ mainly by property values. For example, a `Kitchenware` node model with a
`kind` property can be better than many tiny models if all items share the same
fields and relationships. A `RELATEDTO` edge with `kind`, `confidence`, and
`source` can be better than many loose edge models when the relationship meaning
is intentionally broad.

After choosing granularity, batch related schema definitions and data mutations
together. This keeps refs, validation, rollback, and compact summaries in one
operation.

See [MCP Batch And Graph Patch Requirements](../docs/mcp-batch-graph-patch-requirements.md).
