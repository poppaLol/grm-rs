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
  "command": "node.find User name=\"Alice Jones\" via=out:Authored:Post"
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

## Future Bulk Writes

Agents often create extracted graphs one entity at a time when only
single-operation tools are visible. The planned MCP direction is to add
batch-oriented write affordances alongside the current tools:

- `grm_batch` for ordered structured operation lists
- `grm_graph_patch` for declarative graph-shaped deltas

See [MCP Batch And Graph Patch Requirements](../docs/mcp-batch-graph-patch-requirements.md).
