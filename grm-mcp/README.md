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
