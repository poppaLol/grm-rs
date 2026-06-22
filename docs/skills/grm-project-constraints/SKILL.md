---
name: grm-project-constraints
description: Use when working on grm-rs planning, review, code changes, RFC/standards docs, or project claims and you need to inspect GRM MCP project-memory constraints, policies, decisions, or testing rules before proposing or editing work.
---

# GRM Project Constraints

Before planning or editing GRM, inspect project-memory constraints through the
GRM MCP graph when available.

## Startup

1. Use `grm-project-memory` alongside this skill for MCP orientation,
   inspection, context isolation, mutation safety, and verification.
2. Inspect the constraint, policy, and decision models relevant to the task.

If the schema lacks the models below, use the closest available equivalents and
say what was missing.

## Paramount Live Database Safety

Treat any live database that may contain GRM project memory as protected SOML:
it is shared working memory between the user and agents.

Before destructive, wipe-like, or large benchmark setup/cleanup operations
against a live database, identify whether the target could be shared project
memory. This includes Neo4j, Postgres, Mongo, GRM service workspaces, local
workspace files, mounted Docker volumes, and any configured benchmark database.

Pause for explicit user confirmation before running operations such as:

- `DROP`, `DETACH DELETE`, `DELETE` without narrow filters, `TRUNCATE`, or
  collection/database drops
- fixture reinitialization that overwrites live data
- benchmark cleanup that removes all nodes, relationships, documents, rows, or
  workspace files
- restore/import/load commands that replace existing project memory
- Docker volume deletion or workspace-root cleanup

If there is any doubt whether the database is project memory, stop and ask. A
backup existing is not permission to make restoration necessary.

## Constraint Lookup

Start with these node models:

- `Constraint`
- `Policy`
- `Decision`

Then follow relevant edges where available:

- `REINFORCES_CONSTRAINT`
- `POLICY_HAS_CONSTRAINT`
- `HAS_POLICY`
- `HAS_DECISION`
- `POLICY_APPLIES_TO`
- `REQUIRES_TEST_SURFACE`
- `INFORMS_WORK_SLICE`
- `INFORMS_ROADMAP_ITEM`

Treat these nodes as operating rules, not background notes. If a planned change
conflicts with a constraint or policy, pause and explain the conflict before
editing.

## Testing Policy Shortcut

For testing-related changes, look for:

- `Policy` with title `Tests reinforce code ownership boundaries`
- `Doc` with path `docs/testing-policy.md`
- connected `ImplementationArea` and `TestSurface` nodes

Use the graph to decide where tests belong:

- runtime behavior: `tests/runtime_*.rs`
- CLI/session behavior: CLI or session integration tests
- MCP behavior: `grm-mcp/tests/`
- Python behavior: Python smoke or integration tests
- backend contracts: shared backend integration tests
- stable public JSON/output shapes: integration or golden tests

Inline tests are appropriate only for private helpers, parser edge cases,
private normalization/validation helpers, and internal invariants that are
awkward to reach through a public surface.

## Runtime/Service Boundary Checks

For runtime, MCP, Python, or service-boundary work, also look for constraints
about:

- adapter-only convenience parsing
- canonical structured runtime requests
- write/delete sharing scope
- patch/upsert/merge/multi-match semantics
- service boundary typed operations

Relevant graph models and edges may include:

- `Constraint`
- `Decision`
- `Policy`
- `WorkSlice`
- `RoadmapItem`
- `REINFORCES_CONSTRAINT`
- `INFORMS_WORK_SLICE`
- `INFORMS_ROADMAP_ITEM`
- `POLICY_HAS_CONSTRAINT`

Do not treat adapter ergonomics as the future service contract unless the graph
contains an explicit decision saying so.

## Standards/RFC Documentation Checks

For work on `docs/soml/foundations`, RFCs, protocol standards, or external-facing
GRM/SOML standardization material, also look for graph memory about:

- operations-contract standardization
- draft/proposed protocol status
- capability declarations
- protobuf versioning discipline
- conformance expectations
- implementation truth boundaries

Relevant current graph nodes include:

- `Decision` with title `Frame GRM Protocol RFC as operations-contract standardization`
- `Policy` with title `Standardization docs distinguish draft protocol from implemented service behavior`
- `RoadmapItem` with title `Explore GRM/SOML storage protocol standardization`

Treat RFC material as draft or proposed unless the graph and docs say it has
been accepted. Do not claim auth/TLS, hosted durability, multi-writer
coordination, universal backend portability, conformance, or final standard
status before those are implemented, tested, and accepted.

Keep the distinction clear: standardize the typed graph operations contract, not
backend implementation, physical storage format, or a textual query language.

## Response Pattern

When using this skill, briefly report:

- constraints/policies/decisions inspected
- any conflicts or risks found
- how the constraints affect the plan, tests, or review judgment
