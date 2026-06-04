# MCP Workflow Tutorial

MCP is the agent adapter over typed GRM operations. This tutorial walks through a
session where a human asks an agent to use the GRM MCP service as graph memory,
then asks it to store a small organic chemistry model.

Your mileage may vary. The exact conversation, schema granularity, and tool-call
sequence depend on the model, MCP client, system prompt, user prompting style,
and how much detail the user asks the agent to preserve. Treat this as a
representative session pattern rather than a deterministic transcript.

## Start The Server

Build the MCP server:

```bash
cargo build -p grm-mcp
```

Configure an MCP client to run the compiled binary over stdio:

```json
{
  "mcpServers": {
    "grm": {
      "command": "./target/debug/grm-mcp",
      "args": [
        "--autocommit-json",
        "test-dbs/mcp-chemistry-session.json",
        "--export-json",
        "test-dbs/mcp-chemistry-export.json"
      ]
    }
  }
}
```

The server owns one in-memory `SessionState` per process.
`--autocommit-json` persists successful mutations through the shared
append-log/checkpoint path. `--export-json` keeps a readable
`grm.interchange` graph file updated after successful mutations.

## Ask What GRM Can Do

The human starts with a broad prompt:

```text
You have access to the grm-rs MCP service. Tell me what you can achieve with this.
```

The agent should ask GRM for its own guidance by calling `grm_help`.

A good agent response could look like this:

```text
I can use GRM as a typed local graph memory. I can define node and edge models,
create and update graph data, batch related writes atomically, query nodes and
edges, run CLI-compatible traversal queries, inspect explain/profile output,
inspect backend-maintained indexes, and export the graph as GRM interchange
JSON. For larger writes I should use grm_batch instead of many single create
calls.
```

The agent should also inspect the current schema first by calling `grm_schema_list`.
This can prevent it from overwriting data, or re-interpreting existing data in an
altered schema.

If the server is running with `GRM_BACKEND=neo4j`, the runtime schema reported
by `grm_schema_list` is session-local. The Neo4j graph may already contain data
even when that schema is empty. In that mode, the agent should inspect
`grm://backend/status`; it reports the backend mode, runtime schema model count,
whether the runtime schema is empty, whether local schema memory persistence is
enabled, and whether schema memory was recovered from an existing file. If the
runtime schema is empty, the agent should ask whether to define a fresh schema
or reconstruct one from project docs before writing. Only then should it
perform `grm_batch` writes.

## Ask For Molecules

The human gives the actual task:

```text
I would like you to build a number of organic chemistry molecule descriptions,
like the plastic plug models you see in school but in a graph. Let's see if you
can construct alcohol, petroleum, and citric acid.
```

The agent should clarify the modeling choice in its response. "Alcohol" is a
class of molecules, so the example below models ethanol as the familiar small
alcohol. Petroleum is not one molecule, so the graph represents it as a mixture
containing representative hydrocarbons.

A useful agent response looks like:

```text
I'll model this as a graph of Molecule, Atom, Bond, and Mixture records.
Ethanol will represent the alcohol example. Petroleum will be a Mixture that
contains representative hydrocarbon molecules. Citric acid will be a larger
Molecule with carbon, oxygen, and hydrogen atoms. I'll store atoms as nodes,
bonds as nodes, and connect them with typed edges so the graph can be traversed
like a school ball-and-stick model.
```

This is the point where the agent chooses structured MCP tools as the canonical
path. `grm_query` can help with traversal later, but schema and writes should be
typed JSON calls.

## Plan The Graph

For this tutorial, use a compact schema, but it's worth remembering that this is a
necessarily tightly scoped example:

- `Molecule`: one molecule description, such as ethanol or citric acid
- `Mixture`: a collection of molecules, such as petroleum
- `Atom`: an atom in a molecule, with an element symbol
- `Bond`: a labeled bond with order `1`, `2`, or `3`
- `HAS_ATOM`: connects a molecule to its atoms
- `HAS_BOND`: connects a molecule to its bonds
- `BOND_FROM` and `BOND_TO`: connect a bond node to the two atoms it joins
- `CONTAINS`: connects a mixture to representative molecules

The agent can define that schema in one atomic `grm_batch` call. The real call
would include all node and edge definitions; the excerpt below shows the shape:

```json
{
  "atomic": true,
  "response": "summary",
  "ops": [
    {
      "op": "schema_define_node",
      "args": {
        "name": "Molecule",
        "id_field": "moleculeId",
        "fields": [
          { "name": "name", "type": "string", "required": true },
          { "name": "formula", "type": "string", "required": true },
          { "name": "kind", "type": "string", "required": false }
        ]
      }
    },
    {
      "op": "schema_define_node",
      "args": {
        "name": "Bond",
        "id_field": "bondId",
        "fields": [
          { "name": "label", "type": "string", "required": true },
          { "name": "order", "type": "int", "required": true }
        ]
      }
    },
    {
      "op": "schema_define_edge",
      "args": {
        "name": "HAS_ATOM",
        "from_model": "Molecule",
        "to_model": "Atom",
        "id_field": "hasAtomId",
        "fields": []
      }
    },
    {
      "op": "schema_define_edge",
      "args": {
        "name": "CONTAINS",
        "from_model": "Mixture",
        "to_model": "Molecule",
        "id_field": "containsId",
        "fields": [
          { "name": "role", "type": "string", "required": false }
        ]
      }
    }
  ]
}
```

The omitted operations define `Mixture`, `Atom`, `HAS_BOND`, `BOND_FROM`, and
`BOND_TO` in the same style. Call the full payload with `grm_batch`.

In Neo4j mode, this schema-definition batch is supported and updates only the
current MCP server's session-local runtime schema. The backing Neo4j graph may
outlive that schema metadata, so a future server process must define or
reconstruct schema again before typed reads or writes.

## Store The Molecules

The human then confirms:

```text
Yes, store this in the GRM graph and save it to the configured files.
```

The agent should use `grm_batch` again for the graph data. This example stores a
small ball-and-stick sketch rather than a complete atom-by-atom chemical
database. It uses refs so edges can point at nodes created earlier in the same
batch. The full call can contain dozens of node and edge operations; the excerpt
below shows enough of the pattern:

```json
{
  "atomic": true,
  "response": "detailed",
  "ops": [
    {
      "op": "node_create",
      "args": {
        "ref": "mol:ethanol",
        "model": "Molecule",
        "props": {
          "name": "Ethanol",
          "formula": "C2H6O",
          "kind": "alcohol"
        }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "atom:ethanol:c1",
        "model": "Atom",
        "props": { "label": "ethanol C1", "element": "C" }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "atom:ethanol:c2",
        "model": "Atom",
        "props": { "label": "ethanol C2", "element": "C" }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "atom:ethanol:o1",
        "model": "Atom",
        "props": { "label": "ethanol O1", "element": "O" }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "bond:ethanol:c1-c2",
        "model": "Bond",
        "props": { "label": "ethanol C1-C2", "order": 1 }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "bond:ethanol:c2-o1",
        "model": "Bond",
        "props": { "label": "ethanol C2-O1", "order": 1 }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "mix:petroleum",
        "model": "Mixture",
        "props": {
          "name": "Petroleum",
          "description": "A mixture of hydrocarbons; represented here by sample components."
        }
      }
    },
    {
      "op": "node_create",
      "args": {
        "ref": "mol:citric-acid",
        "model": "Molecule",
        "props": {
          "name": "Citric acid",
          "formula": "C6H8O7",
          "kind": "organic acid"
        }
      }
    },
    {
      "op": "edge_create",
      "args": { "model": "HAS_ATOM", "from": "mol:ethanol", "to": "atom:ethanol:c1", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "HAS_ATOM", "from": "mol:ethanol", "to": "atom:ethanol:c2", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "HAS_ATOM", "from": "mol:ethanol", "to": "atom:ethanol:o1", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "HAS_BOND", "from": "mol:ethanol", "to": "bond:ethanol:c1-c2", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "BOND_FROM", "from": "bond:ethanol:c1-c2", "to": "atom:ethanol:c1", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "BOND_TO", "from": "bond:ethanol:c1-c2", "to": "atom:ethanol:c2", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "HAS_BOND", "from": "mol:ethanol", "to": "bond:ethanol:c2-o1", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "BOND_FROM", "from": "bond:ethanol:c2-o1", "to": "atom:ethanol:c2", "props": {} }
    },
    {
      "op": "edge_create",
      "args": { "model": "BOND_TO", "from": "bond:ethanol:c2-o1", "to": "atom:ethanol:o1", "props": {} }
    }
  ]
}
```

The omitted operations add the remaining ethanol bonds, petroleum component
molecules such as octane and benzene, citric acid atoms and bonds, and the
`CONTAINS` / `HAS_ATOM` / `HAS_BOND` edges that connect them. Call the full
payload with `grm_batch`.

Neo4j mode supports this style of `grm_batch` with `atomic=true` for
`node_create`, `node_update`, `node_delete`, `edge_create`, `edge_update`, and
`edge_delete` operations, including batch-local refs for creates. It still does
not support snapshots, import/export, autocommit, explain/profile, or
traversal/query parity. Graph durability comes from Neo4j; schema metadata
remains session-local.

A good agent response after the batch is:

```text
Stored the chemistry graph. The autocommit session file is
test-dbs/mcp-chemistry-session.json and the interchange export is
test-dbs/mcp-chemistry-export.json. Petroleum is stored as a Mixture, not a
single molecule. Ethanol is the representative alcohol.
```

## Query The Stored Values

The human can now ask:

```text
What molecule did you store for alcohol?
```

The agent should prefer structured lookup:

```json
{
  "model": "Molecule",
  "filters": {
    "kind": "alcohol"
  }
}
```

Call that payload with `grm_node_find`.

To find petroleum components, use structured tools again:

```json
{
  "model": "Mixture",
  "filters": {
    "name": "Petroleum"
  }
}
```

Call that payload with `grm_node_find`, then call `grm_edge_find` with the
returned mixture ID:

```json
{
  "model": "CONTAINS",
  "filters": {
    "from": 4
  }
}
```

Replace `4` with the actual ID returned by `grm_node_find`.

Use `grm_query` only when the agent wants CLI-compatible traversal syntax:

```json
{
  "command": "node.find Molecule name=\"Ethanol\" via=out:HAS_ATOM:Atom"
}
```

Call that payload with `grm_query`.

To inspect the traversal plan instead of parsing query text, call `grm_explain`:

```json
{
  "command": "node.find Molecule name=\"Ethanol\" via=out:HAS_ATOM:Atom"
}
```

To execute and measure the same path, call `grm_profile` with the same payload.

## Save And Export

With the startup configuration above, successful writes already persist to:

```text
test-dbs/mcp-chemistry-session.json
```

They also update the readable interchange file:

```text
test-dbs/mcp-chemistry-export.json
```

The agent can also export on demand:

```json
{
  "path": "test-dbs/mcp-chemistry-export.json"
}
```

Call that payload with `grm_export`.

## Direct Neo4j MCP Mode

The human then asks:

```text
Can you save this straight into Neo4j for me?
```

For a live Neo4j target, start the MCP server in Neo4j mode:

```bash
GRM_BACKEND=neo4j
GRM_SCHEMA_TEMPLATE=project-memory-schema.json
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=...
grm-mcp
```

`GRM_SCHEMA_TEMPLATE` is optional. When set, the server loads the local JSON
file during startup as a GRM session-backed schema memory file. If the file is
missing, the server starts fresh and creates it. If the file exists, the server
recovers the session-local runtime schema from it. Schema definitions made
through the supported Neo4j MCP schema tools are appended to this local file as
they are built. This does not create Neo4j data and does not write schema
metadata into Neo4j. It is useful even for model types that have zero existing
nodes or relationships, because a fresh agent can call `grm_schema_list`
immediately after restart and see the recovered model surface.

This mode is a live graph backend for agent-authored graph memory. It supports
schema-aware mutation and simple lookup through:

- `grm_schema_list`
- `grm_schema_define_node`
- `grm_schema_define_edge`
- `grm_batch` for schema/node/edge create/update/delete operations
- `grm_node_create`
- `grm_node_update`
- `grm_node_delete`
- `grm_edge_create`
- `grm_edge_update`
- `grm_edge_delete`
- simple `grm_node_find`
- simple `grm_edge_find`

It is not a general backend pivot. Runtime schema metadata is session-local in
this first slice: if `grm-mcp` restarts without `GRM_SCHEMA_TEMPLATE`, the Neo4j
graph data remains, but the agent must define the runtime schema again before
finding or extending that data. Agents should still call `grm_schema_list` and
inspect `grm://backend/status` before writes, even when schema memory is
recovered from a local file.
Neo4j durability comes from Neo4j, and GRM snapshot/import, autocommit,
explain/profile, traversal parity, and CLI Neo4j session mode are not part of
this workflow yet.

The built-in help text is conservative and tells agents to ask before inventing
a schema. For autonomous schema-design work, make the permission explicit in
the user prompt:

```text
You may design and define the GRM runtime schema for this Neo4j memory task.
First call grm_schema_list and inspect grm://backend/status. If the runtime
schema is empty or missing required models, choose a compact schema, define it
with grm_batch schema_define_node/schema_define_edge operations, then create the
requested graph data. Do not infer schema from Neo4j labels/properties, and do
not write anything until the runtime schema contains the target models.
```

## Recover From Tool Errors

If a tool call fails and the fix is not obvious, ask for targeted help before
retrying:

```json
{
  "tool": "grm_batch"
}
```

Call that payload with `grm_tool_help`.

Common recovery moves are:

- call `grm_schema_list` when a model, field, or endpoint is uncertain
- use `grm_node_find` or `grm_edge_find` to locate numeric IDs before updates
- use `grm_batch` for related writes so validation and rollback happen together
- use `grm_export` when a user needs a handoff file for another system
- use `GRM_BACKEND=neo4j` when the target is live Neo4j and the workflow fits
  the supported schema/mutation/simple-find tool slice

## Where To Go Next

- Use [MCP batch and graph patch requirements](../mcp-batch-graph-patch-requirements.md)
  for the batch design and planned `grm_graph_patch` direction.
- Use [CLI sessions](cli-session.md) for the equivalent interactive workflow.
- Use [Python sessions](python-session.md) for the same runtime operations from
  `grm_rs.Session`.
