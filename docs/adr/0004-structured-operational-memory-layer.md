# ADR 0004: Frame GRM As A Structured Operational Memory Layer

Status: Accepted

Date: 2026-05-24

## Context

GRM has grown from a runtime graph memory project into a broader runtime model
for applications and agents. It now includes typed schema and graph operations,
CLI sessions, Python bindings, MCP tools, explain/profile support, durability
work, Neo4j compatibility, service API design, and architecture work toward a
future hosted backend.

Describing GRM primarily as a graph database, graph CRUD API, or Cypher
alternative undersells the intended product surface and can pull design toward
database-language and storage-engine comparisons too early.

The graph substrate remains important, but the product direction is the
operational memory/runtime model built on top of it: typed memory, explainable
state resolution, durable transitions, provenance-aware execution, and secure
structured access for applications and agents.

## Decision

GRM will be framed and evolved as a Structured Operational Memory Layer for
applications and agents.

The core product is the operational memory/runtime model itself:

- typed operational memory
- explainable traversal
- durable state transitions
- provenance-aware execution
- secure structured access

Storage engines are implementation details beneath runtime semantics. GRM may
use in-memory storage, Neo4j, future GRM service storage, indexes, acceleration
structures, or other backends, but those implementations should serve the
operational memory model rather than define the product surface.

## Concept Vocabulary

GRM should increasingly use the following conceptual vocabulary when discussing
the product and future architecture:

| Concept | GRM Meaning |
| --- | --- |
| Session | Operational memory context |
| Traversal | Explainable state resolution |
| Delta | Durable operational mutation |
| Projection | Contextual memory surface |
| Attestation | Verifiable operational proof |
| Runtime | Executable memory substrate |

This vocabulary should guide future service, runtime, documentation, UI, sync,
security, and agent-facing work. It does not require immediate renaming of
existing code, tests, or APIs.

## Emerging Stack

The emerging stack is:

| Layer | Responsibility |
| --- | --- |
| SAM | Secure structured access |
| SOML | Operational memory semantics |
| GRM Runtime | Typed traversal/runtime engine |
| Storage Backend | Persistence implementation |
| Sync Layer | Projection/distribution |
| UI Runtime | Reactive graph projection |
| Agent Runtime | Operational memory consumer |

This stack is a product and architecture framing. It should not be treated as a
requirement to create all layers immediately.

## Relationship To Existing Direction

This decision reinforces existing GRM decisions:

- The future service boundary should remain typed and structured, not a textual
  query language.
- CLI-like syntax can remain a human/adapter convenience, but it should not
  become the core service contract.
- Schema memory remains important because agents and humans need orientation
  before extending operational memory.
- Durability work should increasingly describe durable operational deltas and
  recovery, not only snapshot persistence.
- Explain/profile work should increasingly describe explainable state
  resolution, access paths, and execution evidence.
- Future acceleration should remain transparent and explainable beneath typed
  runtime semantics.
- Backend capability differences should be visible and honest; storage engines
  are implementations beneath the runtime model.

## Positioning Guidance

Prefer positioning GRM as:

- a structured operational memory layer
- a typed memory runtime for applications and agents
- a secure structured access layer over operational graph state
- an explainable runtime for resolving and mutating memory state

Avoid positioning GRM primarily as:

- a traditional graph database
- a graph CRUD API
- a Cypher alternative
- a storage engine first

These comparisons may still be useful when explaining implementation choices or
competitive context, but they should not define the product surface.

## Consequences

Positive consequences:

- GRM has a clearer product category that unifies application state, agent
  memory, backend coordination, and governance workflows.
- Service API design can be evaluated by whether it expresses operational
  memory semantics, not whether it resembles a database API.
- Future UI and sync work can be described as projections over operational
  memory rather than generic graph browsing.
- Durability and provenance can become first-class concepts in product and
  architecture discussions.
- Backend choices remain flexible because storage is subordinate to runtime
  semantics.

Tradeoffs:

- New vocabulary such as SOML, SAM, delta, projection, and attestation must be
  introduced carefully to avoid obscuring current implementation reality.
- Existing docs and APIs still use graph/database/CRUD language, and should
  evolve gradually where it improves clarity.
- Some users will still need database-oriented explanations; product language
  should bridge from familiar terms without letting those terms dominate.
- The framing raises expectations around provenance, attestation, projection,
  and secure access that must be made true through design and tests before being
  claimed as delivered.

## Guidance

Future work should follow these rules:

- Keep the service boundary structured and typed.
- Treat graph CRUD as one operational subset, not the whole product model.
- Introduce SOML/SAM vocabulary in docs and graph memory before renaming code.
- Describe durable writes as operational deltas where that improves clarity.
- Make explain/profile part of the trust model for state resolution.
- Keep backend capability claims grounded in implemented and tested behavior.
- Ensure docs for humans and graph memory for agents agree on this framing.

## Open Questions

- What exact service API objects should represent session, traversal, delta,
  projection, and attestation?
- Should SOML and SAM become crate/module names, documentation terms, or both?
- What is the minimum attestation/provenance model that supports credible
  product claims?
- How should projection semantics relate to future UI, sync, and hosted service
  work?
- How should existing graph database language be phased out or contextualized
  without confusing current users?
