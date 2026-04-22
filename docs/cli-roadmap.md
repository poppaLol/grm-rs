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

This means a user can now:

1. bootstrap models and links from a script
2. create and query data interactively
3. save or autocommit the session
4. reload later with schema and data ready to use

## Current Drawbacks

The current CLI is useful, but there are several major limitations:

- query is exact-match only
- command parsing is still based on simple whitespace splitting
- quoted strings and richer literals are not properly supported
- autocommit rewrites the whole session file on each successful change
- persistence is snapshot-based only
- runtime schema is primarily a CLI-layer concept, not yet a deeper core abstraction
- backend identity is only partially abstracted and still effectively `i64`-centric
- the session script format is still a thin command file, not a real DSL
- `src/runtime/session.rs` currently carries too much behavior

## Prioritized Roadmap

### Now

1. Query language expansion
2. Real command parser

### Next

1. Persistence durability improvements
2. Smarter autocommit strategy
3. Session UX polish

### Later

1. Runtime schema and session-core refactor
2. Backend-neutral identity support
3. Stronger script language

### Stretch

1. Import / inference from existing persisted backends
2. Optional code generation from discovered schema

## Detailed Work Items

### Query Language

Improve session query capability beyond exact-match filters.

Target additions:

- comparison operators like `!=`, `>`, `<`, `>=`, `<=`
- string-oriented matching such as `contains`
- limits, ordering, and paging
- traversal-oriented session queries
- clearer node/edge result formatting as query power increases

Guiding rule:
extend the current dotted command style first instead of replacing it immediately.

### Command Parser

Replace the current whitespace-split parser with a real grammar-aware parser.

Target additions:

- quoted string values
- escaped values
- spaces inside property values
- stronger parse errors
- a grammar that works consistently in both REPL and script mode

Guiding rule:
parser work should happen before query syntax becomes much richer.

### Persistence And Autocommit

Improve durability without changing the simple user-facing model.

Target areas:

- reduce or remove full-file rewrite behavior on every autocommit
- evaluate append-log or checkpoint approaches
- improve interrupted-write safety
- define recovery behavior for damaged session files
- keep `session.save`, `session.load`, and `session.autocommit` simple from the user perspective

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

### Persistence And Autocommit

- autocommit no longer depends on rewriting the entire session file every change
- interrupted writes have a defined recovery path
- users can trust saved sessions more like a real workspace

### Architecture

- session command routing is thinner
- runtime schema logic is less CLI-bound
- future backends can realistically plug in different ID models

## Open Questions

These should stay explicit for future planning chats:

- what exact syntax should richer session queries use?
- should scripts remain command files or become a formal DSL?
- should runtime schema converge with compile-time typed model abstractions?
- how should UUID or other non-integer IDs appear in commands and saved sessions?
- how much of the current CLI command shape should be treated as stable?

## Working Principle

The CLI has started revealing real user needs. That is good.

The main risk now is letting CLI convenience outpace core abstractions for too long.

The roadmap should therefore favor:

1. better query capability
2. better parsing
3. better durability
4. cleaner architecture underneath the UX
