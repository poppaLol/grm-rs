# CLI Session Roadmap

## Current State

The `grm session` CLI already supports a useful local workflow:

- runtime `model.define` and `link.define`
- data commands:
  - `node.create`, `node.find`, `node.update`, `node.delete`
  - `edge.create`, `edge.find`, `edge.update`, `edge.delete`
- session commands:
  - `session.save --json|--bin`
  - `session.load --json|--bin`
  - `session.autocommit --json|--bin`
- script bootstrap into an interactive session
- session persistence that restores both graph data and runtime schema
- expanded `node.find` / `edge.find` query syntax:
  - comparison operators: `=`, `!=`, `>`, `<`, `>=`, `<=`
  - string matching with `~`
  - quoted values with whitespace
  - ordering, multi-field ordering, limit, and offset
- a structured command parser shared by REPL and script mode
- explicit output formats for `find`:
  - default human-readable output
  - `format=jsonl`
  - `format=table`

This means a user can now:

1. bootstrap models and links from a script
2. create and query data interactively
3. choose between human-readable and machine-readable find output
4. reload later with schema and data ready to use

## Current Drawbacks

The current CLI is useful, but there are several major limitations:

- traversal-oriented query is not implemented yet
- graph-shaped output is not implemented yet
- coloured terminal output is not implemented yet
- autocommit rewrites the whole session file on each successful change
- persistence is snapshot-based only
- runtime schema is primarily a CLI-layer concept, not yet a deeper core abstraction
- backend identity is only partially abstracted and still effectively `i64`-centric
- the session script format is still a thin command file, not a real DSL
- `src/runtime/session.rs` currently carries too much behavior

## Prioritized Roadmap

### Completed

1. Query language expansion
2. Real command parser

### Now

1. Graph output for graph-shaped and traversal-shaped results
2. Coloured terminal output
3. Session UX polish
4. Traversal-oriented session queries

### Next

1. Persistence durability improvements
2. Smarter autocommit strategy
3. Concurrency and session coordination
4. Python integration surface
5. Explicit bulk-update design for matched query results

### Later

1. Runtime schema and session-core refactor
2. Backend-neutral identity support
3. Stronger script language
4. Pubsub and live subscriptions

### Stretch

1. Import / inference from existing persisted backends
2. Optional code generation from discovered schema

## Detailed Work Items

### Query Language

Status:
completed for the non-traversal phase, with traversal still outstanding.

The CLI now supports richer session queries beyond exact-match filters.

Design note:
see [docs/query-language-design.md](query-language-design.md) for the current grammar sketch, CLI mockups, output-format notes, and acceptance tests.

Completed:

- comparison operators like `!=`, `>`, `<`, `>=`, `<=`
- string-oriented matching via `~`
- limits, ordering, multi-field ordering, and paging
- explicit `find` output formats:
  - default human-readable output
  - `format=jsonl`
  - `format=table`

Outstanding:

- traversal-oriented session queries
- graph-shaped result rendering
- richer graph-aware result display once traversal lands
- explicit bulk-update commands for multi-match query results

Guiding rule:
extend the current dotted command style first instead of replacing it immediately.

Bulk update note:
for now, keep mutations on the current safe model of `node.update <Model> <id> ...` and `edge.update <Link> <id> ...`.
When bulk mutation is introduced, it should be a separate and explicit command family rather than an overloaded extension of `find`, so the CLI can add guardrails such as dry-run counts, confirmations, and clear matched-versus-updated reporting.

### Command Parser

Status:
completed for the current command surface.

The CLI now has a real grammar-aware parser for the current session commands.

Completed:

- quoted string values
- escaped values
- spaces inside property values
- stronger parse errors
- a grammar that works consistently in both REPL and script mode

Follow-on work:

- extend the parser cleanly as traversal syntax is designed
- preserve clear error reporting as the command surface grows

### Output And Presentation

Status:
active; this is the next user-facing focus area.

Current state:

- `find` supports the current human-readable default output
- `format=jsonl` supports machine-readable piping and scripting
- `format=table` supports text-only tabular output

Next additions:

- graph output for graph-shaped and traversal-shaped results
- coloured terminal output for default and table renderers
- decide how colour behaves when output is piped or redirected
- keep non-colour output stable and script-friendly

Guiding rule:
rendering should stay separate from query execution so new formats do not require query rewrites.

### Persistence And Autocommit

Improve durability without changing the simple user-facing model.

Target areas:

- reduce or remove full-file rewrite behavior on every autocommit
- evaluate append-log or checkpoint approaches
- improve interrupted-write safety
- define recovery behavior for damaged session files
- keep `session.save`, `session.load`, and `session.autocommit` simple from the user perspective

### Concurrency And Session Coordination

Status:
not started; this should stay explicit in the roadmap even if the near-term model remains conservative.

Why this matters:

- file-backed sessions can still encounter shared-access behavior
- two CLI users on the same machine or SSH host may try to query or modify the same persisted session
- persistence design choices now will strongly affect how safely that can work later

Target areas:

- decide whether persisted session files are single-writer only
- define file-locking behavior for concurrent CLI processes
- decide whether readers can safely coexist with an active writer
- evaluate transaction-log or append-log approaches for safer recovery and coordination
- define conflict behavior and user-facing errors for concurrent writes
- clarify whether the session model stays local/single-user or grows toward lightweight shared usage

Guiding rule:
prefer explicit and safe coordination semantics over accidental multi-user behavior.

### Pubsub And Live Subscriptions

Status:
not started; this is a later-stage capability that depends on clearer concurrency and session-coordination semantics.

Why this matters:

- live graph updates become much more useful once more than one process or user may observe the same session state
- pubsub creates a path from a local CLI tool toward lightweight shared and reactive workflows
- subscription semantics will influence how traversal, graph rendering, and future automation features feel in practice

Target areas:

- start with simple entity-level pubsub for node and edge changes
- define create, update, and delete event shapes
- decide how subscriptions are scoped:
  - whole graph
  - model/link scoped
  - entity id scoped
- later evaluate query-based subscriptions
- define how query-based pubsub should behave when an entity starts matching or stops matching a query
- decide whether pubsub is CLI-only, library-level, or both
- define how pubsub interacts with file-backed persistence, locks, and future transaction logging

Suggested evolution:

1. entity pubsub for node and edge lifecycle events
2. scoped subscriptions by model, link, or specific ids
3. query-based pubsub for following changes to entities that match a query

Guiding rule:
start with explicit entity events before introducing higher-level query subscriptions.

### Python Integration Surface

Make the current CLI/session work available to Python consumers without forcing the CLI to be the only integration path.

Target areas:

- define a short-term machine-readable CLI flow for experimentation and automation
- design a proper Python binding layer over stable library/session abstractions
- avoid exposing Rust-specific generic and macro-heavy surfaces directly as the first Python API
- prefer Python-friendly dict/list-style inputs and outputs for the initial binding surface
- keep the binding plan aligned with future session-core refactoring instead of baking more behavior into the CLI shell

### Runtime Schema Architecture

Move runtime schema/session behavior out of the CLI shell layer and into cleaner shared abstractions.

Target areas:

- separate command handling from session/domain logic
- reduce how much behavior accumulates in `src/runtime/session.rs`
- define runtime schema as a stronger engine-level concept
- clarify the relationship between:
  - runtime CLI schema
  - persisted session schema
  - compile-time Rust `NodeModel` / `RelModel`

### Backend-Neutral Identity

Make backend-owned IDs truly backend-neutral rather than only conceptually abstracted.

Target areas:

- remove hidden assumptions that IDs are always `i64`
- define how CLI parsing/rendering should work for non-integer IDs
- prepare for UUID-backed backends
- make persistence and query paths support that abstraction consistently

### Script Language Evolution

Decide whether script files remain command lists or become a real DSL.

Possible future features:

- includes
- variables
- better comments and formatting rules
- grouped setup blocks
- optional transaction boundaries
- more structured execution feedback

### Session UX And Packaging

Keep making the CLI feel like a real product surface.

Target areas:

- better help and onboarding
- cleaner error messages
- history and line-editing support
- completion support later
- install/distribution polish
- example scripts that demonstrate the happy path clearly

### Long-Term Import / Inference / Codegen

Treat this as a stretch track, not a near-term implementation area.

Potential future direction:

- infer models and links from existing SQL / Neo4j / MongoDB-style persisted data
- load discovered schema into the runtime session layer
- optionally generate Rust model code from inferred schema

## Acceptance Signals

The near-term work is in a good place when:

### Query And Parser

- users can express more than exact-match filters
- quoted values work reliably in REPL and scripts
- parser errors are precise and actionable
- the command language stays readable

These are now largely satisfied for the current non-traversal query surface.

### Output And Presentation

- users can choose between default, `jsonl`, and `table` output
- `jsonl` remains reliable for scripting and piping
- graph output clearly communicates traversal structure
- coloured output improves readability without harming pipe-friendly behavior

### Persistence And Autocommit

- autocommit no longer depends on rewriting the entire session file every change
- interrupted writes have a defined recovery path
- users can trust saved sessions more like a real workspace

### Concurrency And Coordination

- concurrent access behavior is documented and predictable
- conflicting writers fail safely and clearly
- any file-locking or transaction-log strategy is visible to users through understandable errors

### Pubsub And Live Updates

- entity-level subscriptions have stable event shapes
- subscription scope is understandable and testable
- query-based subscriptions have clear semantics for entering, leaving, and updating matches

### Architecture

- session command routing is thinner
- runtime schema logic is less CLI-bound
- future backends can realistically plug in different ID models

## Open Questions

These should stay explicit for future planning chats:

- what exact syntax should richer session queries use?
- what exact traversal syntax should the CLI adopt?
- what should graph output look like for branching traversals?
- what colour rules should apply for interactive terminals vs redirected output?
- should file-backed sessions be treated as strictly single-writer?
- what coordination model should exist for two CLI sessions pointed at the same file?
- when, if ever, should transaction logging grow into multi-user coordination rather than just recovery?
- what transport or mechanism should power pubsub for local and shared sessions?
- should pubsub begin as entity events only, with query subscriptions later?
- what exact semantics should query-based subscriptions use when a matching set changes over time?
- should scripts remain command files or become a formal DSL?
- should runtime schema converge with compile-time typed model abstractions?
- how should UUID or other non-integer IDs appear in commands and saved sessions?
- how much of the current CLI command shape should be treated as stable?

## Working Principle

The CLI has started revealing real user needs. That is good.

The main risk now is letting CLI convenience outpace core abstractions for too long.

The roadmap should therefore favor:

1. better output and traversal capability
2. better durability
3. cleaner architecture underneath the UX
4. stronger backend neutrality over time
