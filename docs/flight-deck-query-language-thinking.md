# Flight-Deck Query Language Thinking

## Context

The flight-deck now has the first shape of a query/audit workbench:

- Query is the main graph-output arena.
- Audit is a separate navigable panel.
- Explain/Profile are visible alongside Query, but currently as local
  client-side summaries.
- `GraphCanvas` can switch between `Data` and `Schema`, but the Query controls
  do not currently understand that switch.

The next design question is whether the flight-deck should expose:

- a structured/query-builder UI;
- CLI-like GRM syntax;
- Cypher;
- both GRM syntax and Cypher; or
- a staged path where one is supported first and the other remains a deliberate
  future adapter.

A newer product thought changes the likely sequence:

1. Add CLI-like GRM syntax first, mainly to prove the typed query execution
   mechanism in the cockpit.
2. Then hook the flight-deck to an agent over MCP so a user can talk about
   their data, schema, explain/profile output, and audit context.
3. Treat Cypher as a possible later adapter, not the next proof.

## Current Product Constraint

GRM project memory records `Structured operations are canonical`.

That means any text syntax in flight-deck should be treated as an adapter over
typed GRM operations. The service/runtime boundary should continue to receive
structured schema, CRUD, traversal/query, explain/profile, batch, durability,
and future admin request objects.

Supporting a textual syntax in the browser does not need to violate this, but
only if the text is parsed/compiled into the existing structured request model
before it crosses the service boundary.

## Schema View Versus Data View

Switching the graph between `Schema` and `Data` should change what the query
engine presents.

### Data View

Data queries should operate on graph instances:

- find nodes by model and properties;
- find edges by model, endpoints, and properties;
- traverse from root nodes through one or more relationship hops;
- choose return shape: root, end, edge, or later subgraph/path;
- explain/profile the actual data operation.

This maps well onto existing GRM concepts:

- `NodeFindRequest`
- `EdgeFindRequest`
- traversal-shaped `node.find`
- `ExplainRequest`
- `ProfileRequest`

### Schema View

Schema queries should operate on model/catalog structure:

- list node models;
- list edge models;
- find models by name or field;
- inspect model fields;
- inspect legal relationship directions;
- show which edge models connect selected node models;
- later: compare declared versus inferred schema or drift.

This is not the same as data traversal. Schema view probably wants a different
small query surface, such as:

```text
schema.find model~User
schema.find field=name
schema.edges from=User
schema.edges to=Post
schema.path User Post
```

or a query-builder form over the same ideas.

## Existing Code Findings

### CLI Syntax Already Has A Good Adapter Path

The CLI parser in `src/runtime/parser.rs` already parses commands such as:

```text
node.find User name=Ada via=out:Authored:Post end.title=Traversal
edge.find Authored from=1
session.explain node.find User name=Ada
session.profile edge.find Authored from=1
```

The runtime then converts CLI/query terms into structured query state in
`src/runtime/session.rs`.

`src/runtime/ops.rs` already has adapter helpers:

- `NodeFindRequest::from_adapter_query_terms`
- `NodeFindRequest::from_adapter_filter_values`
- `EdgeFindRequest::from_adapter_filter_values`

This makes a flight-deck GRM-syntax text adapter relatively plausible:

1. Accept a read-only subset of CLI-like text.
2. Parse only `node.find`, `edge.find`, `session.explain`, and
   `session.profile`.
3. Compile to `NodeFindRequest`, `EdgeFindRequest`, `ExplainRequest`, or
   `ProfileRequest`.
4. Send only structured requests through the gateway/service path.

The main work is product/UI shaping, gateway endpoint design, and ensuring the
parser subset is read-only and clear.

### Service Protocol Already Has Structured Query Shapes

`grm-service-api/proto/grm/service/v1/query.proto` already defines:

- `NodeFindShape`
- `EdgeFindShape`
- `TraversalStep`
- `QueryRequest`
- `ExplainRequest`
- `ProfileRequest`

`workspace.proto` includes these under `WorkspaceRuntimeRequest` through
workspace-scoped execution.

This means the service contract already has the right canonical target. The
flight-deck should not need to introduce free-form query text as a service
contract.

### GRM Can Emit Cypher, But Does Not Parse Cypher

`src/backend/cypher.rs` translates internal `GraphQuery` values into Cypher
text with parameters. Tests in `tests/cypher_translation.rs` cover this
direction.

That is useful, but it is the opposite of what a Cypher input box needs.

Current capability:

```text
GRM structured query -> Cypher
```

Cypher input would require:

```text
Cypher text -> parsed AST -> restricted GRM structured query
```

There is no obvious existing Cypher parser in the repo.

## Cypher Support: Initial Difficulty Assessment

### Easy Version: Not Really Cypher

Difficulty: small to medium.

Support only a few Cypher-looking templates and compile them into existing
structured requests.

Examples:

```cypher
MATCH (u:User) RETURN u LIMIT 25
MATCH (u:User)-[:Authored]->(p:Post) RETURN p LIMIT 25
MATCH (u:User)-[r:Authored]->(p:Post) RETURN r LIMIT 25
```

This could be implemented with a deliberately small parser and very strict
errors. It would be useful for familiarity, but it should not be described as
general Cypher support.

Risks:

- users expect real Cypher semantics;
- schema labels/types need exact mapping to GRM model names;
- WHERE support quickly expands scope;
- RETURN shape must remain bounded to supported node/edge result shapes.

### Useful Restricted Cypher

Difficulty: medium to large.

Support a documented read-only subset:

- single `MATCH`;
- one root node;
- zero or more fixed relationship hops;
- labels and relationship types;
- simple `WHERE` predicates with `=`, `<>`, `<`, `<=`, `>`, `>=`, `CONTAINS`;
- `RETURN` one node or one relationship;
- `LIMIT` and `SKIP`;
- maybe `ORDER BY` over returned item fields.

This maps to current GRM traversal-shaped `NodeFindRequest` and `EdgeFindRequest`
reasonably well, but still requires:

- a lexer/parser or parser dependency;
- AST validation;
- variable binding checks;
- model/type validation against runtime schema;
- clear unsupported diagnostics;
- tests for unsupported Cypher features;
- redaction and bounded execution behavior;
- explain/profile integration.

This is probably the smallest Cypher path that would feel honest.

### General Cypher

Difficulty: large to very large.

General Cypher includes features far beyond current GRM structured requests:

- multiple `MATCH` clauses;
- optional match;
- variable-length paths;
- path returns;
- arbitrary expressions;
- aggregations;
- maps/lists;
- functions;
- `WITH`;
- `UNWIND`;
- writes such as `CREATE`, `MERGE`, `SET`, `DELETE`;
- subqueries;
- planner semantics that users associate with Neo4j.

Supporting this would either require embedding/delegating to a Cypher engine or
building a substantial language implementation. It would also risk repositioning
GRM as a Cypher alternative, which conflicts with current project direction.

## Recommendation

Do not start with Cypher.

The next engineering slice should make the query workbench useful by
supporting a read-only GRM syntax adapter that mirrors the CLI subset:

```text
node.find <Model> ...
edge.find <Model> ...
session.explain node.find <Model> ...
session.profile node.find <Model> ...
session.explain edge.find <Model> ...
session.profile edge.find <Model> ...
```

This path has the best leverage because it reuses existing parser/runtime
concepts and compiles naturally to canonical structured requests.

The CLI-like syntax should be framed as a mechanism proof, not the final human
experience. The more compelling product move after that is an agent hook-up:
let the user ask questions about the loaded workspace through MCP, while the
agent uses structured GRM tools and the same typed query/explain/profile
surface underneath.

In parallel, design the query mode switch explicitly:

- `Data query`: node/edge/traversal operations.
- `Schema query`: model/field/relationship inspection.

## Agent Hook-Up After CLI Syntax

The product path that now looks strongest is:

```text
typed query workbench -> CLI-like mechanism proof -> MCP agent chat
```

The flight-deck should not try to make every user learn CLI syntax. The CLI-like
adapter is valuable because it proves that text can compile to typed GRM
requests and return graph/explain/profile/audit context. Once that mechanism is
real, the natural user experience is conversational:

```text
Which work slices are blocked by security questions?
Show me the schema around WorkSlice and SecurityRequirement.
Why did this query return only three nodes?
What changed since the last snapshot?
Find the shortest explanation path from Admin no.1 bootstrap to audit risk.
```

The agent should answer by calling MCP/GRM tools, not by scraping UI state or
inventing query semantics. The flight-deck can provide context such as selected
workspace, selected node/edge, graph view mode, current schema, current query,
and last explain/profile result.

### Why This Is Different From The Deferred Short Agent Demo

The earlier Short Agent Memory Demo was deferred because live-agent
tool-orchestration reliability was not strong enough to gate cockpit work.

This proposed agent hook-up should come later, after the cockpit has:

- a real typed query execution path;
- a schema/data mode distinction;
- explain/profile output;
- bounded audit/workbench context; and
- a stable MCP surface the agent can use directly.

That makes the agent an assistant over proven cockpit semantics, not the proof
of the semantics itself.

### Likely Architecture

Keep the direction MCP-native and typed:

- flight-deck remains the human cockpit;
- GRM service/runtime remains the typed operational memory boundary;
- MCP exposes structured tools for agents;
- the agent translates user intent into GRM tool calls;
- results come back as graph selections, query results, explain/profile, and
  audit/context cards.

Possible minimal integration shapes:

- `Open in agent` / `Ask agent about this graph` sends selected workspace,
  selected item, current schema/data mode, and current query context to an MCP
  client/session.
- A local sidecar provides a chat panel that can call the existing GRM MCP
  server.
- The flight-deck records agent-driven tool calls as bounded audit/workbench
  events.

### Guardrails

- Do not let the agent bypass the same authorization/security status that
  normal typed service calls use.
- Do not expose private key paths, certs, permission tables, credentials, raw
  fingerprints, or unbounded graph payloads to browser state.
- Do not market this as autonomous correctness or vulnerability sign-off.
- Keep agent answers grounded in returned GRM data, explain/profile output, and
  explicit unsupported states.
- Keep CLI/Cypher text as adapters; the agent should prefer structured MCP
  tools where available.

Then revisit Cypher as a documented read-only adapter once the workbench has a
real typed query execution path and real explain/profile output.

If Cypher is added later, describe it as:

```text
Cypher-like read-only adapter for a documented GRM-supported subset
```

not:

```text
GRM supports Cypher
```

unless and until the implemented/tested behavior is broad enough to justify
that claim.

## Possible Next Prompt

Create an engineer prompt for:

```text
Flight-deck typed query workbench with data/schema modes and CLI-like GRM syntax
```

Acceptance should include:

- graph view state moves out of `GraphCanvas` so Query controls know whether
  the user is in Data or Schema mode;
- Data mode supports a CLI-like read-only query text area or command bar for
  `node.find` and `edge.find`;
- Schema mode has different controls for schema/model inspection;
- query text compiles to typed GRM requests, not free-form service text;
- explain/profile use real service/runtime responses where supported;
- Cypher is explicitly deferred or shown only as an unsupported future adapter.

Then create a later PM/architecture prompt for:

```text
Flight-deck MCP agent chat over typed workspace query context
```

Acceptance should include:

- the agent uses MCP/structured GRM tools rather than free-form service text;
- the chat context is bounded to selected workspace/schema/data/query state;
- agent responses cite or link the graph/query/explain/profile evidence used;
- user-visible results can update the graph selection or query panel;
- security and audit boundaries remain read-only unless a later explicit
  admin-boundary design exists.
