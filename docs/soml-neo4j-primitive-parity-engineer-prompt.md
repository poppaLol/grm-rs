# GRM Neo4j Primitive-Type Parity Engineer Prompt

## Objective

Implement the next bounded follow-on to ADR 0013: make Neo4j mode preserve and
query GRM's six extended primitive types losslessly and with the same logical
semantics as the in-memory/service-backed runtime:

- `bytes`
- `decimal`
- `date`
- `datetime`
- `duration`
- `uuid`

Success means supported Neo4j schema, create, read, update, predicate, and
ordering paths round-trip canonical GRM typed values without lossy coercion or
backend-specific semantics leaking through the public contract.

## Required Skills

Use these repository skills before planning or editing:

- `grm-architect`
- `grm-project-constraints`
- `grm-project-memory`

Use `grm-product-manager` when checking scope, roadmap state, or updating graph
memory after completion. Inspect the live GRM project graph before making
backend capability or parity claims.

## Starting Context

Read these first:

- `docs/adr/0013-establish-soml-primitive-value-types.md`
- `docs/soml/data-type-primitives.md`
- `src/runtime/value.rs`
- `src/runtime/neo4j.rs`
- `src/backend/neo4j.rs`
- `src/runtime/session.rs`
- `grm-mcp/src/service.rs`
- existing Neo4j and shared backend tests

Treat the canonical logical value contract in ADR 0013 and
`src/runtime/value.rs` as authoritative. Neo4j physical representation is an
adapter concern.

## Scope

1. Define explicit lossless Neo4j property mappings for all six types.
2. Reconstruct and validate canonical tagged GRM values on reads.
3. Support create, update, equality/comparison predicates, and ordering wherever
   the logical type defines those operations.
4. Keep schema metadata outside the user's Neo4j graph by default, following
   the accepted project decision.
5. Update capability/help/error reporting so it states exactly what is now
   supported and preserves explicit errors for anything still unsupported.
6. Reuse shared runtime canonicalization and comparison behavior where
   practical; do not create a second type contract in the Neo4j adapter.

Choose representations based on losslessness and predictable query behavior,
not merely Neo4j convenience. If one type cannot be mapped faithfully, retain
an explicit unsupported capability for that type and document the evidence
rather than coercing it.

## Non-Goals

- Cypher as a public GRM query surface or arbitrary raw-Cypher execution.
- Visual projection or flight-deck work.
- New primitive, container, nullable, refinement, or graph-native types.
- Schema migration or rewriting existing stored fields.
- Persisting GRM schema metadata inside user graph data.
- Claiming full Neo4j parity for traversal, import/export, explain/profile, or
  other unrelated backend capabilities.
- Destructive testing against the live project-memory Neo4j database.

## Acceptance Criteria

- Each supported extended primitive round-trips through Neo4j with its logical
  type and canonical value intact.
- Node and relationship create/read/update paths are covered.
- Equality, applicable range predicates, and ordering agree with shared runtime
  behavior, including exact decimal and chronological datetime cases.
- Invalid or mismatched values fail before mutation with stable GRM errors.
- Existing `string`, `bool`, `int`, and finite `float` behavior remains intact.
- MCP Neo4j mode exposes the supported behavior through structured tools and
  reports remaining capability limits honestly.
- Tests run against an isolated disposable Neo4j instance or narrowly scoped
  fixtures. Never reset, clear, migrate, or bulk-delete the live project-memory
  database.

## Verification

Place tests at their ownership boundaries:

- private codec/mapping invariants may use focused unit tests;
- portable backend behavior belongs in shared backend contract tests;
- Neo4j integration behavior belongs in Neo4j integration tests;
- MCP-visible behavior belongs in `grm-mcp/tests/`.

Run Cargo commands sequentially. Before handoff, run the focused Neo4j/backend
tests, relevant MCP tests, the affected workspace tests, `cargo fmt --check`,
and `cargo clippy --workspace --all-targets -- -D warnings`. Record any live
Neo4j test that could not be run; do not replace it with an unsupported parity
claim.

## Completion Record

When implementation and review are complete, update ADR 0013's implementation
status and the live GRM WorkSlice/roadmap memory with the precise supported
Neo4j type and operation matrix. Keep partial support explicit.
