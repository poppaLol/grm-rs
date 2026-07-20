# Agent Memory Demo

This demo shows the smallest useful GRM operational-memory story:

1. an agent running through Claude Code inspects typed schema memory;
2. it records a project decision, a work slice, and a constraint;
3. the GRM service and MCP adapter restart;
4. the agent reconnects to the same service-managed workspace; and
5. traversal and explain evidence show what it remembered and why.

The commands below make the complete setup reproducible, including service
startup, initial storage, restart, and grounded recall. Runtime varies with the
model, hardware, and client harness.

## What This Demonstrates

This demo proves a bounded local claim:

- an agent can write typed operational memory through structured MCP tools;
- a service-managed workspace preserves schema, nodes, and relationships across
  service restart;
- a fresh MCP process can reopen that workspace and orient from its schema;
- traversal can recall connected project context; and
- explain output can expose the access path used for recall.

## The Memory Shape

The example stores three typed records:

```text
Decision:  Use file-plus-directory audit acceptance
WorkSlice: Implement durable local audit log store
Constraint: Do not overstate durability
```

Two typed relationships preserve their meaning:

```text
Decision ──INFORMS──> WorkSlice
Decision ──REINFORCES──> Constraint
```

This is deliberately smaller than a general project-management schema. The
point is to demonstrate durable, connected, explainable memory—not to freeze a
universal planning ontology.

## Build The Service And MCP Adapter

From the repository root:

```bash
cargo build -p grm-service-api -p grm-mcp
```

Use a dedicated local service root so the demonstration is isolated from other
GRM workspaces:

```bash
GRM_SERVICE_SECURITY_PROFILE=anonymous_local \
target/debug/grm-local-workspace-server \
  127.0.0.1:50051 /tmp/grm-agent-memory-demo
```

## Configure Claude Code

Save the following configuration as `.mcp.json` in the repository root.
Claude Code automatically discovers that project-scoped file when it is started
from the same directory and asks for approval before using the server. The
configuration runs `grm-mcp` over stdio with the gRPC workspace backend. Its
relative command path is resolved from that working directory; use an absolute
path if starting Claude Code elsewhere.

```json
{
  "mcpServers": {
    "grm-agent-memory-demo": {
      "transport": "stdio",
      "command": "./target/debug/grm-mcp",
      "env": {
        "GRM_BACKEND": "grpc",
        "GRM_SERVICE_ENDPOINT": "http://127.0.0.1:50051",
        "GRM_WORKSPACE_REF": "agent-memory-demo",
        "GRM_SERVICE_WORKSPACE_MODE": "create-or-open"
      }
    }
  }
}
```

Three workspace modes are available:

- `create` requires a new workspace and fails if the reference already exists;
- `open` requires an existing workspace and is the default when the variable is
  omitted; and
- `create-or-open` creates the workspace when absent and otherwise reopens it.

This onboarding demo uses `create-or-open` so it can be run once or twice
without editing the MCP configuration. For a stricter persistence test, use
exact `create` on the first run and exact `open` after restart. Neither mode
silently replaces an existing workspace.

## Ask The Agent To Orient And Remember

Give the agent running in Claude Code this prompt:

```text
Use GRM as typed project memory for this demonstration.

First call grm_help and grm_schema_list so you understand the available tools
and any schema or data already present. Do not assume the workspace is empty
and do not create duplicate records when this demo is run again.

Model this compact project memory:

- A Decision titled “Use file-plus-directory audit acceptance”. Its summary
  should say that a local audit record is accepted only after its complete
  record, file synchronization, and required directory synchronization
  succeed.
- A WorkSlice titled “Implement durable local audit log store” with status
  “completed”.
- A Constraint titled “Do not overstate durability”. Its summary should limit
  claims to tested local, single-process, single-writer filesystem behavior.
- The Decision INFORMS the WorkSlice.
- The Decision REINFORCES the Constraint.

Use the node models Decision, WorkSlice, and Constraint and the directed edge
models INFORMS and REINFORCES so the recall step can traverse them by name.
Choose a compact typed schema with the fields needed to preserve the content
above. If the required schema is missing, define it before writing data. If
matching nodes or relationships already exist, reuse and verify them rather
than creating duplicates.

Use structured GRM MCP tools for schema inspection, schema definition, writes,
and verification. Prefer one atomic grm_batch for related schema changes and
one atomic grm_batch for related data changes when new records are required.
Do not use textual query commands for schema or writes.

After writing, verify each node by its exact identifying field and verify both
relationship directions and endpoints. Once finished, tell me a little about
what we have stored and how it is connected.
```

The prompt specifies the intended memory and stable model names, while leaving
schema construction, batching, endpoint resolution, and verification to the
agent using the MCP service. A successful first phase ends with three verified
nodes and two verified relationships.

### Watch The Service Log

Keep the gRPC service terminal visible during the demonstration. Its structured
operation log provides immediate evidence that the agent is calling through the
MCP adapter into the workspace service rather than merely describing proposed
actions. A first run can look like:

```text
workspace_operation completed workspace=agent-memory-demo operation=workspace.create
workspace_operation completed workspace=agent-memory-demo operation=schema.list
workspace_operation completed workspace=agent-memory-demo operation=batch
workspace_operation completed workspace=agent-memory-demo operation=batch models_defined=3 links_defined=2
workspace_operation completed workspace=agent-memory-demo operation=node.find
workspace_operation completed workspace=agent-memory-demo operation=node.find
workspace_operation completed workspace=agent-memory-demo operation=node.find
workspace_operation completed workspace=agent-memory-demo operation=batch nodes_created=3
```

The exact sequence can vary when the workspace already exists or the agent
chooses different verification calls. These entries corroborate that service
operations completed and summarize some effects; they do not by themselves
prove the stored property values, relationship endpoints, or restart recovery.
The typed read-back and post-restart recall below remain the acceptance checks.

## Restart The Memory Service

This is the proof boundary. Stop the MCP adapter and the GRM service. Restart
the service with the same service root:

```bash
GRM_SERVICE_SECURITY_PROFILE=anonymous_local \
target/debug/grm-local-workspace-server \
  127.0.0.1:50051 /tmp/grm-agent-memory-demo
```

Reconnect or restart Claude Code with the same `create-or-open` configuration.
Because the workspace now exists, the adapter opens it. This produces a fresh
MCP process and a fresh service process while preserving the service-managed
workspace.

If you selected the stricter lifecycle variant, change
`GRM_SERVICE_WORKSPACE_MODE` from `create` to `open` before reconnecting.

## Ask The Recall Question

Give the fresh agent this prompt:

```text
Orient from GRM schema memory. What decision informs the durable-audit work
slice, and which durability constraint does that decision reinforce?

First call grm_schema_list. Use typed grm_node_find traversal to retrieve the
stored records. Then call the grm_explain MCP tool for the first traversal to
show its database access plan. Here, "explain" specifically means the plan
returned by grm_explain, not a narrative explanation of a proposed query.

Do not recreate the schema or data, invent examples, or merely suggest commands
for me to run. Base the answer only on records returned by GRM. Finish with a
concise text explanation rather than returning the tool-call JSON objects.
```

The agent should first call `grm_schema_list`. It can recall the decision from
the work slice with a structured traversal-shaped `grm_node_find` call:

```json
{
  "model": "WorkSlice",
  "filters": {
    "title": "Implement durable local audit log store"
  },
  "via": ["in:INFORMS:Decision"],
  "return": "end",
  "limit": 5
}
```

It can then traverse from the returned decision to the constraint:

```json
{
  "model": "Decision",
  "filters": {
    "title": "Use file-plus-directory audit acceptance"
  },
  "via": ["out:REINFORCES:Constraint"],
  "return": "end",
  "limit": 5
}
```

To expose the first access path, call `grm_explain` with the equivalent
CLI-compatible read command:

```json
{
  "command": "node.find WorkSlice title=\"Implement durable local audit log store\" via=in:INFORMS:Decision return=end limit=5"
}
```

Textual command syntax is used here only as the current explain input. Schema
and writes remain structured typed operations.

## Expected Result

A concise agent response should look like:

```text
The durable-audit work slice is informed by the decision “Use
file-plus-directory audit acceptance.” That decision reinforces the constraint
“Do not overstate durability,” which limits the claim to tested local,
single-process, single-writer filesystem behavior.

GRM recovered the workspace schema after restart, traversed WorkSlice
<-INFORMS- Decision, then Decision -REINFORCES-> Constraint. Explain confirms
the bounded typed traversal used for the first lookup.
```

That is the product demonstration: the agent did not rely on chat history. It
reoriented from durable typed memory, followed explicit relationships, and
showed why the answer was returned.

## Repeat Or Reset

For normal repetition, leave the service-managed workspace in place and use
`GRM_SERVICE_WORKSPACE_MODE=create-or-open`. Use exact `open` when a missing
workspace should be treated as an error rather than initialized.

The directory under `/tmp` is disposable demo state, but do not automate broad
cleanup against an arbitrary configured service root. Confirm the exact target
before removing or replacing any workspace data.

## Demo Caveats

### Use A Capable Tool-Calling Model

This workflow requires a model that can reliably call MCP tools with structured
arguments. Conversational ability alone does not show that a model can select
tools, construct schema-aware payloads, follow a multi-step write workflow, or
continue correctly from tool results.

The demonstrated MCP path was tested with Claude Code configured to use models
served both on the same machine and elsewhere on the local network. This showed
that the client harness and its tool-call orchestration matter alongside model
capability. Local and LAN-hosted models can make the prepared sequence
significantly slower; a capable cloud-hosted model is the more predictable
choice when turnaround time is important to the live demonstration.

The workflow was also attempted with `ollmcp` and several Ollama models,
including `qwen2.5-coder:32b`. Some runs selected tools or completed individual
steps, but the full storage, restart, grounded recall, and database-plan evidence
could not be proven reliably through that harness. Smaller models often failed
to follow the initial prompt or invoke tools correctly. These observations do
not establish a universal parameter-count threshold, and they do not identify
the model alone as the cause.

Typical signs that a model or agent setup is not suitable include:

- describing operations without calling a tool;
- defining the schema but declining or forgetting to store data;
- inventing tool names or producing malformed arguments;
- printing raw tool-call objects instead of interpreting their results;
- losing the task after the first schema or lookup response; and
- repeatedly failing when tool calling is invoked.

These failures are not evidence that GRM failed to persist or traverse memory.
Before the full demo, confirm that the model can list the GRM tools and call
`grm_help` and `grm_schema_list`. For a broader introduction to schema design,
batching, backend differences, Neo4j, and error recovery, use the
[MCP workflow tutorial](mcp-workflow.md).

### Scope And Security

This demo does not prove hosted durability, multiple writers, public-service
security, attestation, non-repudiation, or recovery from disk loss or operator
deletion. A remote single-VM run can demonstrate deployment repeatability, but
does not turn the local service into a hosted, multi-writer, or publicly secured
GRM offering.

The `anonymous_local` profile used above is an explicitly weaker development
profile. Keep it bound to loopback; exposing the service remotely requires a
separately designed secured deployment profile.
