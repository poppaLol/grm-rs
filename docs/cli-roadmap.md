# GRM CLI Roadmap

This roadmap describes where the `grm session` experience is going next. It is
intentionally future-facing: current commands and behavior should live in the
README, tutorials, or focused design notes.

The CLI is the most visible GRM workflow today. It is also a proving ground for
runtime schema, persistence, backend contracts, query introspection, Python
bindings, and MCP agent workflows.

Guiding question:

> Can a user or agent build, inspect, persist, and exchange graph-shaped project
> knowledge without giving up a clear path to typed Rust models and real graph
> backends?

## Current Priorities

1. Continue Python and MCP surface parity, especially remaining schema/CRUD
   polish and shared error behavior.
2. Improve local in-memory backend durability, including safer autocommit,
   recovery behavior, and WAL evaluation.
3. Prepare session-core and runtime-schema cleanup so the CLI shell becomes
   thinner and more behavior moves into shared library paths.
4. Build demo scenarios that show equivalent CLI, Rust, Python, and MCP
   workflows over the same graph-shaped model.

## Near-Term Direction

### Python And MCP Parity

The Python and MCP surfaces should feel like first-class ways to use GRM, not
thin wrappers over CLI text.

Near-term work:

- align remaining schema and CRUD operations across CLI, Python, and MCP
- keep shared batch operations as the preferred multi-entity write path
- normalize error shapes and recovery hints across integration surfaces
- make agent-facing help steer repeated writes toward batch or graph patch tools

Related notes:

- [MCP batch and graph patch requirements](mcp-batch-graph-patch-requirements.md)
- [Python quickstart](python-quickstart.md)
- [Python Neo4j API expansion](python-neo4j-api-expansion.md)

### Local Durability

Local sessions should become safer and more operationally honest. The user model
can stay simple, but the implementation needs clearer durability boundaries.

Near-term work:

- define the target durability class for local file-backed sessions
- reduce full snapshot rewrites in autocommit paths where practical
- evaluate a compact operation-delta WAL based on transaction deltas
- preserve clear recovery behavior for damaged snapshots and replay logs
- benchmark save, load, compact, replay, and file-size growth separately

Related notes:

- [Durability testing](durability-testing.md)
- [Query and persistence optimization](query-persistence-optimization.md)

### Session-Core Cleanup

`src/runtime/session.rs` has carried a lot of product behavior while the CLI was
evolving quickly. The next architecture step is to move stable semantics into
shared runtime/session modules that Python and MCP can use directly.

Near-term work:

- separate command routing from session/domain behavior
- make runtime schema a stronger library concept
- keep parser, rendering, persistence, import/export, and batch semantics in
  clearer modules
- preserve the CLI's dotted command style while reducing CLI-specific coupling

### Demo Workflows

The project needs examples that make its shape obvious.

Good demo scenarios should show:

- a small typed Rust model
- the same runtime model in the CLI
- Python session usage
- MCP batch or graph patch usage
- query, traversal, explain/profile, save/export, and reload

## Next

1. Concurrency and session coordination
2. Import/export design and bulk interchange surface across CLI, Python, and MCP
3. Explicit bulk-update design for matched query results
4. Richer traversal result controls and graph presentation polish
5. Backend-neutral identity support beyond the current mostly-`i64` shape
6. Grow the live Neo4j backend from prototype toward a fuller Cypher-compliant backend
7. Resilient Redis-like local backend behavior: append-friendly durability, recovery, compaction, and operational tooling

## Later

1. Runtime schema and session-core refactor
2. Stronger Python/library integration surface
3. Stronger script language
4. Pubsub and live subscriptions
5. Link directionality semantics, including bidirectional-by-default link types
6. Additional language integrations such as a C# LINQ provider where a concrete workflow justifies them

## Stretch

1. Import / inference from existing persisted backends
2. Optional code generation from discovered schema

## Design Principles

### Keep The CLI Understandable

The current dotted command style remains the default shape for the session CLI.
New behavior should extend that style before replacing it.

### Separate Workflows From Storage

`session.save` and `session.load` are workspace persistence commands.
`session.import` and `session.export` are interchange commands. They may share
internal machinery, but their user promises should stay distinct.

### Prefer Explicit Bulk Mutation

Bulk mutation should not be hidden behind `find`. Future bulk update commands
should provide dry-run counts, clear matched-versus-updated reporting, and
guardrails for destructive operations.

### Make Backend Contracts Portable

Backend behavior should be tested through shared suites where possible. In-memory
and Neo4j behavior should diverge only where backend capabilities explicitly say
they do.

### Make Performance Inspectable

`session.explain` and `session.profile` are first-phase introspection tools. They
should grow carefully toward better planning and profiling without overstating
what the engine currently does.

### Favor Durable Local Workflows

GRM can stay local-first and lightweight while still being honest about recovery,
single-writer assumptions, and interrupted-write behavior.

## Important Open Questions

- What exact durability class should local sessions promise?
- Should file-backed sessions remain strictly single-writer?
- What transaction model should a future service-style backend expose?
- How should UUID or non-integer backend IDs appear in CLI commands and saved
  sessions?
- How much of the current dotted command surface should be treated as stable?
- When should scripts become more than command files?
- How should graph workspaces eventually represent separate memory/project
  segments without complicating the current user model?

## Related Documents

- [Query language design](query-language-design.md)
- [Query and persistence optimization](query-persistence-optimization.md)
- [Durability testing](durability-testing.md)
- [Import/export](import-export.md)
- [MCP batch and graph patch requirements](mcp-batch-graph-patch-requirements.md)
- [Python quickstart](python-quickstart.md)
