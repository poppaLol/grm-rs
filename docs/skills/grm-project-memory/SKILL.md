---
name: grm-project-memory
description: Use whenever a GRM skill or grm-rs task reads, traverses, creates, updates, or verifies live project memory through GRM MCP. Provides shared MCP orientation, bounded structured inspection, product-context isolation, mutation safety, error recovery, and write verification for the protected SOML graph.
---

# GRM Project Memory

Use this companion skill for the mechanics and safety of working with the live
GRM project graph. Use the active specialist skill to decide which domain
models, relationships, documents, and implementation evidence matter.

## Startup

1. Call `grm_help` when first using the server in a session.
2. Call `grm_schema_list` before querying or writing unfamiliar models.
3. In Neo4j mode, read `grm://backend/status` and `grm://graph/summary`.
4. If schema memory was recovered from a template, verify that its models and
   fields match the intended operation. Recovered schema does not prove that
   matching graph data exists.
5. If runtime schema is empty, ask whether to define or reconstruct it before
   making typed reads or writes.

If graph access is unavailable, say so and follow the specialist skill's
degraded-mode guidance. Do not present graph-backed direction as confirmed.

## Evidence Hierarchy

Keep these evidence classes distinct:

- code and public tests establish current implementation behavior;
- graph memory records project context, accepted direction, constraints,
  decisions, risks, and reported work status;
- docs and ADRs record declared design and claims but may lag implementation or
  graph decisions; and
- aspirational graph nodes and documents describe intended future state, not
  delivered capability.

When sources disagree, identify the mismatch and inspect the most relevant
implementation or decision evidence. Do not silently treat graph presence,
documentation, or planned status as proof that behavior exists.

## Bounded Inspection

- Prefer structured MCP tools over `grm_query` or embedded Cypher.
- Find nodes with `grm_node_find`, preferring a known id or exact identifying
  field such as `name`, `title`, `path`, or `question` and a bounded limit.
- Inspect connections with `grm_edge_find`, selecting one declared edge model
  and filtering by `from` or `to` endpoint id.
- Follow paths one declared relationship at a time: resolve the root, inspect
  its relevant edges, then resolve endpoint nodes.
- Use paging and ordering when a bounded model scan is genuinely required.
- Call `grm_tool_help` for the failing tool before retrying an unsupported
  filter, traversal, or malformed operation.

In Neo4j MCP mode, do not assume structured traversal, `grm_query`, explain,
profile, import, export, or other local-mode capabilities are available. Use
the capabilities reported by `grm_help` and fall back to bounded node and edge
lookups.

## Context Isolation

- Identify the relevant `ProductContext` before maintaining roadmap or domain
  memory.
- Reach project nodes through declared context, roadmap, slice, or domain
  relationships rather than thematic similarity.
- Before creating an edge, resolve both endpoints and confirm that the edge
  model, direction, and product context are intentional.
- Do not connect leaf nodes across product contexts merely because they share a
  technology, security concern, person, or repository.
- Create an explicit cross-context relationship model only after the user has
  accepted that shared meaning as part of the schema.

## Safe Mutation

Treat the graph as protected shared SOML. Use `grm-project-constraints` for the
full live-database safety policy.

- Inspect existing nodes and edges before creating replacements or duplicates.
- Use only fields and relationship models declared by `grm_schema_list`.
- Prefer individual structured tools for up to three narrow changes.
- Prefer atomic `grm_batch` for more than three related creates or updates;
  pass operation objects directly and use batch-local refs for new endpoints.
- Do not enable deletes or perform broad cleanup without explicit user
  approval and a narrowly verified target.
- Keep graph property values to supported scalar strings, numbers, or booleans.

### Schema Discipline

Keep to the declared runtime schema and the existing graph shape.

- Do not attempt to create invalid nodes, invalid edges, invalid properties, or placeholder models as probes.
- If the intended model, edge, field, or direction is missing, stop and ask the user whether to extend the schema.
- Use read-only discovery first: `grm_schema_list`, `grm_node_find`, `grm_edge_find`, tool help, and MCP resources.
- Treat write-shaped calls as mutations even when you expect them to fail.
- Do not checkpoint schema memory unless the log is too large or the user has indicated that checkpointing is wanted.

## Verification

After writes:

1. Re-read each changed node by id or exact identifying field.
2. Re-read each changed relationship by model and endpoint id.
3. Use `grm://graph/summary` for count-level confirmation when useful.
4. Use `grm_export` only when the active backend reports it as supported.
5. Report created or updated ids and any capability-limited verification.

If a Neo4j Browser view appears incomplete or disconnected, check its display
limit and verify the specific entities and relationships through MCP before
concluding that a write is missing.
