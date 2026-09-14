# WS564 Engineer Prompt: Split MCP Batch Write And Destructive Tools

You are the engineer agent for WorkSlice 564, "Split MCP batch write and destructive tools."

## Objective

Replace the ambiguous MCP batch affordance with explicit non-destructive and destructive batch tools while preserving the existing shared runtime batch executor, atomic behavior, durability behavior, and deliberate compatibility for the current `grm_batch` surface.

The outcome should make ordinary create/update batches the obvious safe path for agents, and make delete-bearing batches visible to MCP hosts as destructive before the tool is invoked.

## Project Memory Context

Use the GRM project-memory graph before changing code:

- Inspect WorkSlice `564`.
- Inspect Constraint `4`, "Structured operations are canonical."
- Inspect Policy `54`, "Tests reinforce code ownership boundaries."
- Inspect Risk `587`, "Admin-plane mutation could sneak in through bootstrap ergonomics."

Implications for this slice:

- Structured operation objects remain the canonical interface. Do not make agents compose CLI strings or generic "call any tool" payloads.
- MCP-facing behavior belongs in `grm-mcp/tests/`, with smaller runtime tests only where shared runtime helpers change.
- Classify destructive operations on the server side from typed operations. Do not trust any caller-supplied classification, actor, permission, policy, or audit outcome.
- Keep project-memory/SOML safety intact. Do not add broad cleanup tools or schema-memory destructive behavior as part of this work.

## Current Starting Point

Read these files first:

- `docs/mcp-batch-graph-patch-requirements.md`
- `grm-mcp/src/tools.rs`
- `grm-mcp/src/schema.rs`
- `grm-mcp/src/help.rs`
- `grm-mcp/src/server.rs`
- `grm-mcp/src/service.rs`
- `src/runtime/batch.rs`
- `src/runtime/neo4j.rs`
- `grm-mcp/tests/stdio.rs`
- `docs/security/threat-model.md`

Important current behavior:

- `grm_batch` accepts `schema_define_node`, `schema_define_edge`, `node_create`, `node_update`, `node_delete`, `edge_create`, `edge_update`, and `edge_delete`.
- Deletes are currently guarded by `allow_deletes`.
- Runtime batch operation classification already has `SessionBatchOp::is_delete()`.
- In-memory, Neo4j, and service-backed MCP modes all route through typed batch operations.
- MCP tools already use safety annotations such as `read_only_hint`, `idempotent_hint`, and `open_world_hint`.

## Implementation Scope

Add an explicit non-destructive batch tool.

Recommended name: `grm_batch_write`.

Behavior:

- Accept the same structured operation shape as `grm_batch`.
- Allow schema definition, node create/update, and edge create/update.
- Reject `node_delete` and `edge_delete` before execution or side effects.
- Ignore or reject `allow_deletes` when present; prefer a clear error if `allow_deletes=true` is supplied to the non-destructive tool.
- Preserve `atomic`, `response`, batch-local refs, grouped counts, validation error shape, persistence, and rollback semantics for accepted operations.
- Route through the existing runtime, Neo4j, and service batch machinery after preflight classification.

Add an explicit destructive batch tool.

Recommended name: `grm_batch_destructive`.

Behavior:

- Accept the same structured operation shape as `grm_batch`.
- Permit delete-bearing batches only through this tool.
- Still require `allow_deletes=true`, or an equivalent explicit typed confirmation field if you choose to rename it for the new tool.
- Advertise MCP destructive metadata using the RMCP tool annotation support available in this repo. In MCP terms, this should be exposed as a destructive tool, not merely described in prose.
- Keep `open_world_hint=false`.
- Do not introduce broad delete, filter delete, wildcard delete, detach/cascade delete, schema delete, workspace delete, import reset, or cleanup semantics.

Keep `grm_batch` compatible deliberately.

Behavior:

- Do not silently narrow existing `grm_batch` semantics.
- Existing callers that use `grm_batch` with `allow_deletes=true` should continue to work unless a test or existing design doc proves otherwise.
- Update help/docs so `grm_batch` is described as the compatibility/general batch surface, while new callers are steered to `grm_batch_write` for ordinary create/update batches and `grm_batch_destructive` for deletes.

## Suggested Code Shape

Prefer a small shared helper over duplicating batch logic.

One workable shape:

- Add a `BatchSafety` or `BatchMutationClass` helper near `SessionBatchOp`.
- Classify each operation from the typed enum before execution.
- Add MCP wrapper methods that validate the requested tool's safety mode, then call the existing in-memory, Neo4j, or service batch path.
- If service mode needs the same preflight behavior, classify in `grm-mcp` before converting to protobuf and invoking `ExecuteWorkspace`.
- Keep the runtime executor as the source of atomic application and rollback behavior.

Make sure the safe wrapper rejects all destructive operations before the first operation is applied, including when `atomic=false`.

## Help And Docs

Update agent-facing guidance so agents see the safer path:

- `grm_help`
- `grm_tool_help`
- `known_tools`
- `grm://docs/agent-guide` output if affected
- `docs/tutorials/mcp-workflow.md`
- `docs/mcp-batch-graph-patch-requirements.md`
- Any MCP README or support matrix touched by the current tool list

Guidance should say:

- For more than 3 creates or updates, prefer `grm_batch_write`.
- Use `grm_batch_destructive` only when the intended batch includes `node_delete` or `edge_delete`.
- `grm_batch` remains available for compatibility/general batch behavior.

## Tests

Add or update MCP tests in `grm-mcp/tests/stdio.rs` for:

- Tool listing exposes `grm_batch_write` and `grm_batch_destructive`.
- `grm_batch_write` accepts a representative schema/create/update batch.
- `grm_batch_write` rejects `node_delete` and `edge_delete` before any side effects, even with `atomic=false`.
- `grm_batch_write` rejects or clearly ignores `allow_deletes=true`; prefer rejection because it catches confused callers.
- `grm_batch_destructive` exposes destructive MCP annotations.
- `grm_batch_destructive` still requires explicit delete confirmation and preserves the target entity when missing.
- `grm_batch_destructive` deletes correctly when explicitly confirmed.
- Existing `grm_batch` delete behavior remains covered as intentional compatibility.
- Streamable HTTP tool listing preserves the new safety annotations if the existing HTTP annotation tests cover tool metadata.

If the classification helper moves into runtime, add focused runtime tests there too. Keep broader behavior assertions at the MCP boundary.

## Security Notes

This is not an admin-plane permissions slice.

Do not add:

- principal mutation
- access-level mutation
- workspace membership mutation
- caller-supplied authorization decisions
- signed receipt changes
- service API permission semantics

Do ensure:

- destructive classification is server-derived from typed operations
- safe-batch rejection happens before any side effect
- destructive tool metadata and explicit delete confirmation are both enforced
- existing delete controls, schema validation, transaction behavior, and durability outcomes remain intact

## Acceptance Criteria

- Agents and MCP hosts can distinguish ordinary batch writes from destructive batches at tool-discovery time.
- Non-destructive batch execution cannot perform deletes through `node_delete` or `edge_delete`.
- Delete-bearing batch execution remains available through an explicitly destructive tool with explicit confirmation.
- `grm_batch` compatibility remains intentional and tested.
- In-memory, Neo4j, and service-backed MCP modes keep consistent behavior.
- Help/docs steer new work toward `grm_batch_write` and reserve `grm_batch_destructive` for delete-bearing batches.
- Tests prove no partial side effects occur when the non-destructive tool rejects a mixed batch.

## Verification

Run targeted checks first:

```bash
cargo test -p grm-mcp
cargo test --test runtime_session
```

If shared runtime batch code changes materially, also run:

```bash
cargo test --test runtime_batch
```

If service conversion or `ExecuteWorkspace` routing changes, also run:

```bash
cargo test -p grm-service-api
```

Before handoff, run the narrowest useful clippy check that covers touched crates, for example:

```bash
cargo clippy -p grm-mcp --all-targets -- -D warnings
```

Do not run destructive database setup or cleanup against a live Neo4j database unless the target is clearly isolated and the command is explicitly approved.

## Handoff Notes

In the final engineer report, include:

- the exact new tool names
- how `grm_batch` compatibility was preserved
- where destructive MCP metadata is asserted
- which modes were tested: in-memory, Neo4j, service-backed, or mocked
- any tests not run and why

After merge, update project memory for WS564 with the implemented tool names and keep Risk `587` active unless a separate admin-plane design actually resolves it.
