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
  - `format=graph`
- traversal-oriented `node.find` queries with chained `via=...` hops
- coloured CLI output and improved script summaries

This means a user can now:

1. bootstrap models and links from a script
2. create and query data interactively
3. traverse related persisted data from the session query surface
4. choose between default, `jsonl`, `table`, and `graph` find output
5. get colour-aware interactive output on supported terminals
6. reload later with schema and data ready to use

## Current Drawbacks

The current CLI is useful, but there are several major limitations:

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
3. Traversal-oriented session queries
4. Graph output for graph-shaped and traversal-shaped results
5. Coloured terminal output
6. Session UX polish

### Now

1. Persistence durability improvements
2. Smarter autocommit strategy
3. Python integration surface improvements
4. Session-core cleanup and runtime/schema refactor prep

### Next

1. Concurrency and session coordination
2. Import/export design and bulk interchange surface
3. Explicit bulk-update design for matched query results
4. Richer traversal result controls and graph presentation polish
5. Backend-neutral identity support

### Later

1. Runtime schema and session-core refactor
2. Stronger Python/library integration surface
3. Stronger script language
4. Pubsub and live subscriptions
5. Link directionality semantics, including bidirectional-by-default link types

### Stretch

1. Import / inference from existing persisted backends
2. Optional code generation from discovered schema

## Detailed Work Items

### Query Language

Status:
completed for the current session query surface, including the first traversal phase.

The CLI now supports richer session queries beyond exact-match filters.

Design note:
see [docs/query-language-design.md](query-language-design.md) for the current grammar sketch, CLI mockups, output-format notes, and acceptance tests.

Completed:

- comparison operators like `!=`, `>`, `<`, `>=`, `<=`
- string-oriented matching via `~`
- limits, ordering, multi-field ordering, and paging
- traversal-oriented `node.find` with chained `via=...` hops
- traversal-scoped `end.*` and `edge.*` / `rel.*` filters
- traversal return controls via `return=root|end|edge`
- explicit `find` output formats:
  - default human-readable output
  - `format=jsonl`
  - `format=table`
  - `format=graph`

Outstanding:

- explicit bulk-update commands for multi-match query results
- richer traversal controls beyond the first session-level traversal shape
- graph presentation polish for denser or more complex traversal results

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
partially completed, with presentation polish still active.

Current state:

- `find` supports the current human-readable default output
- `format=jsonl` supports machine-readable piping and scripting
- `format=table` supports text-only tabular output
- `format=graph` supports graph-shaped and traversal-shaped session results
- coloured output is available for the interactive CLI surface

Next additions:

- decide how colour behaves when output is piped or redirected
- keep non-colour output stable and script-friendly
- refine graph rendering for more branching and denser result sets

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

### Import / Export Surface

Status:
planned, with the current direction now clearer than the implementation.

Core direction:

- keep `.grm` as the human-authored script format for setup, examples, demos, and tests
- keep `session.save` / `session.load` focused on restoring a local workspace snapshot
- add `session.import` / `session.export` later as a separate interchange-oriented command family

Likely format split:

- `JSON` as the default structured interchange format for full graph or session-style bundles
- `JSONL` for larger streaming-oriented exports and imports
- binary as the speed/size-oriented local persistence format rather than the main cross-tool interchange format

Implementation bias:

- avoid replaying bulk imports through the one-command-at-a-time CLI path
- parse and validate in batches
- create nodes and edges in batches
- avoid per-object transaction and rendering overhead where possible

Guiding rule:
separate workspace persistence semantics from interchange semantics, even if some internal representations overlap.

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

Sequencing note:
directionality features such as bidirectional-by-default link types should wait until after durability/logging and multi-user coordination decisions are clearer, since shared semantics and recovery behavior will affect how safe those features are to introduce.

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

### Link Directionality Semantics

Status:
not started; this is a near-term but not immediate follow-on area.

Why this matters:

- some relationship types are naturally symmetric from a user perspective
- the current model stores links with explicit `from` and `to` semantics only
- traversal already supports `out`, `in`, and `both`, but link definitions do not yet express whether a link type should behave as directed or bidirectional by default

Target areas:

- decide whether link definitions should support an explicit directionality setting
- decide whether "bidirectional" means symmetric traversal semantics, automatic mirror-edge creation, or both
- keep query and rendering behavior understandable when a link type is treated as bidirectional
- make sure persistence, recovery, and concurrent-write behavior remain safe once these semantics exist

Guiding rule:
do not introduce bidirectional-by-default link behavior until durability/logging and multi-user coordination semantics are settled enough to support it cleanly.

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
- example scripts that demonstrate the happy path clearly
- improved script summaries and interactive readability
- cleaner error messages
- history and line-editing support
- completion support later
- install/distribution polish

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
