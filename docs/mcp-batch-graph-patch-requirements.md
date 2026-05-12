# MCP Batch And Graph Patch Requirements

Status: requirements draft

The initial MCP implementation exposes clear single-operation tools such as
`grm_node_create`, `grm_edge_create`, `grm_node_update`, and `grm_edge_update`.
That is useful for small edits, but agents naturally follow visible affordances:
if the only obvious write tools create one entity at a time, agents will tend to
insert graphs node-by-node and edge-by-edge.

For larger graph construction and update workflows, MCP should expose explicit
batch-oriented affordances and make them the recommended path.

## Goals

- make efficient multi-entity writes obvious to agents
- reduce repeated MCP round trips for graph construction
- preserve transactional semantics for related writes
- return compact summaries instead of large per-entity responses by default
- support both operation-oriented batching and graph-shaped patching
- leave existing single-entity tools in place for small/manual edits

## Non-Goals

- do not remove or deprecate the existing single-operation tools
- do not make agents compose CLI strings as the primary bulk path
- do not require a new persistence/WAL design before adding the MCP affordance
- do not implement backend-neutral identity as part of the first batch feature

## Proposed Tool Split

### `grm_batch`

`grm_batch` applies an ordered list of existing MCP-style operations. Its
operation semantics now live in the shared runtime batch helper, which is also
used by Python `Session.batch(...)`; MCP keeps the tool schema and wrapper.

Use it when the caller knows the exact sequence of operations:

- define schema
- create nodes
- create edges
- update nodes
- update edges
- delete nodes or edges when `allow_deletes` is explicitly set

The input should be structured JSON rather than CLI text. Agents are generally
better at producing JSON operation objects than safely composing command strings.

Sketch:

```json
{
  "atomic": true,
  "response": "summary",
  "ops": [
    {
      "op": "schema_define_node",
      "args": {
        "name": "File",
        "id_field": "fileId",
        "fields": [
          { "name": "path", "type": "string", "required": true }
        ]
      }
    },
    {
      "op": "node_create",
      "args": {
        "model": "File",
        "props": { "path": "src/lib.rs" }
      }
    }
  ]
}
```

Expected behavior:

- `atomic: true` applies all operations or none.
- `atomic: false` may apply successful operations and return indexed failures.
- validation errors include the failing operation index.
- summary responses include counts grouped by operation and model.
- detailed responses can include created/updated/deleted IDs when requested.
- edge create operations can use numeric node ids already known to the caller, or
  batch-local refs from earlier `node_create` operations.
- batch-local refs must be unique within the batch.
- delete operations are rejected unless `allow_deletes` is true.

### `grm_graph_patch`

`grm_graph_patch` should apply a declarative graph delta.

Use it when the caller has a desired graph-shaped change rather than a sequence
of low-level operations. This is a better fit for agents extracting knowledge
from source files, documents, repositories, or conversations.

Sketch:

```json
{
  "atomic": true,
  "response": "summary",
  "nodes": {
    "create": [
      {
        "ref": "file:src/lib.rs",
        "model": "File",
        "props": { "path": "src/lib.rs" }
      }
    ],
    "update": [
      {
        "model": "File",
        "match": { "path": "src/lib.rs" },
        "props": { "summary": "Library entrypoint" }
      }
    ]
  },
  "edges": {
    "create": [
      {
        "model": "Contains",
        "from_ref": "file:src/lib.rs",
        "to_ref": "item:SessionState",
        "props": {}
      }
    ]
  }
}
```

Expected behavior:

- patch-local `ref` values allow edges to target nodes created earlier in the
  same patch.
- update operations should support match filters, but ambiguous matches must be
  rejected unless an explicit multi-match mode is added later.
- create-or-update/upsert semantics should be considered, but should be explicit
  rather than inferred from partial input.
- merge-oriented behavior should reduce noisy entity-by-entity updates by
  letting agents refer to existing or newly-created data through explicit refs,
  match filters, or upsert rules.
- the result should include compact counts and a ref-to-ID map when useful.

## Agent Guidance Requirements

When these tools are added, agent-facing guidance should actively steer bulk
work toward them.

Required guidance changes:

- `grm_help` should say to prefer `grm_batch` or `grm_graph_patch` when creating
  or updating more than a few entities.
- `grm_node_create`, `grm_edge_create`, `grm_node_update`, and `grm_edge_update`
  descriptions should mention the batch/patch tools for repeated operations.
- `grm_tool_help` for single-entity write tools should include a batching note.
- `known_tools` should group these under a new `bulk` or `batch` category.
- `grm_batch` and `grm_graph_patch` responses should be more token-efficient
  than many single-operation calls, so agents receive a practical reward for
  using them.

Suggested threshold language:

> For more than 3 creates or updates, prefer `grm_batch` or `grm_graph_patch`.

## Result Shape Requirements

Bulk tools should default to compact summaries.

Minimum summary fields:

- `applied`: boolean
- `atomic`: boolean
- `operation_count`: integer
- `counts`: grouped counts by operation and model
- `errors`: array with operation/patch index, message, and recovery hint
- `ids`: optional created/updated IDs or patch refs when `response` requests it

The response should avoid returning every full entity unless requested.

## Transaction And Persistence Requirements

- Successful atomic batches should persist once, after the whole batch succeeds.
- Failed atomic batches should not persist partial changes.
- Non-atomic batches should persist only after successful applied operations,
  preferably once at the end.
- `--export-json` updates should also happen once per successful batch/patch,
  not once per child operation.

## Implementation Slot

The current MCP layout can absorb this cleanly:

- add parameter structs and custom JSON schemas in `grm-mcp/src/schema.rs`
- add tool handlers in `grm-mcp/src/tools.rs`
- route operations through shared internal helpers rather than re-entering MCP
  tool handlers
- update `grm-mcp/src/help.rs` with the new tools and batching guidance
- add stdio tests in `grm-mcp/tests/stdio.rs`

The first implementation should prefer simple, explicit operation enums over a
fully generic "call any tool by name" format. That keeps validation tight and
avoids accidentally exposing persistence or load/import operations inside bulk
mutation batches.

## Initial Acceptance Tests

- `grm_batch` creates multiple nodes in one call and returns grouped counts.
- `grm_batch` creates nodes and edges using numeric IDs supplied by the caller
  or batch-local refs from earlier `node_create` operations.
- failed `atomic: true` batch leaves the session unchanged.
- failed non-atomic batch reports partial success with operation indexes.
- duplicate batch-local refs are rejected.
- delete operations require `allow_deletes: true`.
- `grm_graph_patch` creates a small connected graph using local refs.
- `grm_graph_patch` rejects ambiguous updates unless explicit multi-match mode
  is supplied.
- successful bulk calls trigger autocommit/export once.
- help text nudges agents toward bulk tools for repeated writes.
