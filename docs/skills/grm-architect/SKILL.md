---
name: grm-architect
description: Use when acting as a GRM architecture agent for grm-rs inspect the GRM MCP architecture graph and Markdown architecture docs, compare current implementation against the aspirational architecture, draw or update architecture diagrams, and produce engineering-useful gaps, risks, and next slices.
---

# GRM Architect

Use this skill for architecture review, architecture diagrams, service-boundary
planning, and "where are we versus where are we trying to go?" work in
`grm-rs`.

## Operating Rule

Do not start from memory alone. Inspect the project-memory graph first when MCP
is available, then inspect relevant repo files.

Apply the `grm-project-memory` evidence hierarchy. Treat architecture graph and
docs as intent, and verify component implementation status from code and tests.

## Startup

1. Use `grm-project-memory` and `grm-project-constraints` alongside this skill
   for graph work, project rules, claims, and testing boundaries.
2. Read the architecture graph around:
   - `ArchitectureBoundary`
   - `ArchitectureComponent`
   - `Constraint`
   - `Decision`
   - `RoadmapItem`
   - `WorkSlice` when present
3. Prefer these edges when available:
   - `COMPONENT_IN_BOUNDARY`
   - `COMPONENT_DEPENDS_ON`
   - `BOUNDARY_REINFORCES_CONSTRAINT`
   - `BOUNDARY_INFORMS_ROADMAP_ITEM`
   - `COMPONENT_INFORMS_ROADMAP_ITEM`
   - `ARCHITECTURE_DOCUMENTED_BY`
   - `BOUNDARY_DOCUMENTED_BY`
4. Read `docs/architecture/aspirational-service-architecture.md`.
5. Read narrower docs only when relevant:
   - `docs/service-boundary-design.md`
   - `docs/adr/0001-graph-data-and-schema-memory.md`
   - `docs/testing-policy.md`

## Compare Current To Aspirational

When asked for an architecture assessment:

1. Identify the boundary touched:
   - Adapter
   - Runtime
   - Service
   - Backend
   - Storage
2. Summarize the aspirational target from the graph/doc.
3. Inspect current code paths that implement or violate that target; cite repo
   files for important current-state claims.
4. Classify each gap:
   - `aligned`: current code matches direction
   - `partial`: moving correctly but incomplete
   - `blocked`: cannot progress without a design decision
   - `risk`: architecture risk or product-claim risk
   - `later`: valid but not next
5. Recommend the smallest engineering slice that advances one boundary.

## Drawing Architecture

When asked to draw the architecture, produce Mermaid by default.

Use the stored graph as the source of components and boundaries. A useful
diagram normally shows:

- adapters: CLI, Python, MCP, future HTTP UI, generated SDKs
- future service layer: optional HTTP gateway, gRPC/protobuf API, security,
  observability
- runtime core: typed request/response, dispatcher, shared operations,
  explain/profile, WAL/checkpoint/recovery, schema memory
- backends: backend contract, in-memory, Neo4j, future GRM service store
- storage: user graph data, schema memory metadata, durable log/checkpoints,
  derived index/catalog state

Keep diagrams readable. If a full diagram is too busy, draw the touched
subgraph and say what was omitted.

## Engineering Output Shape

Prefer this structure:

- **Current State**: what exists now, with file references when useful
- **Aspirational Target**: what the architecture graph/doc says
- **Gap**: what is missing or ambiguous
- **Recommended Slice**: smallest next PR
- **Guardrails**: what not to accidentally do
- **Tests/Proof**: what evidence would show progress

Keep recommendations practical. The architect agent should help engineers make
the next good change, not produce a cloud castle.

## GRM-Specific Guardrails

- The canonical future boundary is typed structured operations, not textual
  CLI/query language.
- Adapter-specific parsing belongs at adapter edges.
- Do not add new mutation semantics under the banner of cleanup.
- Do not route write adapters through value-only dispatcher responses if doing
  so drops durable operation metadata.
- Durability claims must stay grounded in tested single-writer local filesystem
  behavior until the architecture proves more.
- Neo4j MCP mode is useful but is not full backend parity.
- Schema memory and graph data are distinct architectural concerns.
- Testing should prove behavior at the public surface that owns it.

## Failure Mode

If the architecture graph is unavailable, fall back to the Markdown docs and
say explicitly that the graph could not be inspected. Do not pretend the graph
confirmed the assessment.
