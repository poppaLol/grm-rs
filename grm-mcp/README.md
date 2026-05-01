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
        "/workspaces/grm-rs/test-dbs/query-playground.export.json"
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
grm://schema
grm://graph/export
grm://graph/summary
grm://docs/query-language
```
