# GRM Cockpit UI Technology Comparison

Date: 2026-07-27
Updated: 2026-07-28

## Status

Working direction accepted. This is not yet an ADR.

## Context

The GRM roadmap calls for a restrained, read-oriented cockpit over typed
service/runtime operations: workspace orientation, schema memory, graph
inspection, traversal/explain evidence, backend status, and security context.

The cockpit must remain an adapter over typed GRM operations. It must not turn
browser command text, UI-local state, or graph-widget behavior into a canonical
service contract.

Three implementation paths have now been spiked:

- Native Rust: `grm-cockpit-egui` using `eframe` and `egui_graphs`.
- Rust browser/WASM: `grm-cockpit-web` using Leptos and Cytoscape.js.
- Mainstream TypeScript browser: `grm-cockpit-react` using React, Vite, and
  Cytoscape.js.
- Spatial scene engine: `grm-cockpit-bevy` using Bevy.

All browser options currently use the existing read-only
`grm-cockpit-web-gateway`, which exposes one bounded snapshot read over the
typed gRPC workspace path.

## Summary Recommendation

Use a browser cockpit with a mainstream TypeScript frontend as the default
workbench direction. Vue/Vite and React/Vite are both acceptable candidates for
the cockpit shell; React/Vite is the current reference spike, while Vue/Vite
remains a credible equivalent if preferred for product UI ergonomics.

Build 2D first for schema, graph inspection, query/explain/profile, audit, and
security context. Preserve the execution-animation idea as an event-stream
layer that can drive both 2D highlighting and later spatial views.

Keep the shared bounded snapshot model and the narrow HTTP adapter. These are
the important architectural choices. The UI framework is replaceable as long as
it consumes typed, bounded GRM projections and does not invent backend
semantics.

Do not continue investing in the native egui path as the main cockpit unless
the product explicitly prioritizes a desktop-only developer workbench. Keep it
as useful learning and possibly as a native diagnostic tool.

Do not promote the Leptos/WASM path yet unless Rust-only frontend ownership is
more valuable than contributor familiarity, package ecosystem, and browser UI
velocity.

Treat the Bevy path as a separate spatial-memory explorer experiment. It is not
a substitute for a dense operational workbench, but it may be useful for
demonstrations, cluster/path exploration, and future embodied memory-map ideas.

The accepted direction is therefore:

> Vue/Vite or React/Vite for the cockpit workbench; 2D first for schema, graph
> inspection, query/explain/profile, audit, and security context; keep execution
> animation alive as an event-stream layer.

## Local Spike Evidence

Approximate source size in the current spikes:

| Surface | Main local files | Approximate source lines | Notes |
| --- | ---: | ---: | --- |
| Native egui | `src/lib.rs`, `src/main.rs` | 288 | Includes the shared gRPC snapshot loader and native UI. |
| Leptos/Cytoscape | `src/main.rs`, JS bridge, CSS | 220 | Compact, but split across Rust/WASM plus manual JS interop. |
| React/Cytoscape | React components, types, API, CSS | 509 | More verbose, but more explicit and familiar to web contributors. |
| Bevy spatial explorer | scene, layout, sample snapshot | 377 | Native and WASM checks pass, but it is a scene-engine experiment rather than an admin UI. |
| Shared gateway/model | gateway + model | 84 | Correctly small; should remain the contract layer for browser spikes. |

The React spike builds with `npm run build`. The production bundle warns at
about 643 kB minified / 205 kB gzip, mostly expected for Cytoscape and acceptable
for an internal cockpit spike. This should be measured again once real cockpit
views exist.

The egui spike has Rust integration test coverage for loading a snapshot through
the workspace gRPC boundary. The browser spikes currently prove build and manual
interaction shape; they need UI-level regression checks before sign-off.

The Bevy spike loads the same `CockpitSnapshot` shape from JSON, maps node
models to color groups, places nodes in deterministic 3D space, draws edges
with Bevy gizmos, and provides orbit/zoom camera controls. `cargo check`,
`cargo test`, and a `wasm32-unknown-unknown` check pass for the crate. The WASM
check requires the explicit `getrandom` web backend flag documented in the
spike README.

## Relative Evaluation

Scoring: 1 is best/lowest risk, 5 is worst/highest risk.

| Criterion | egui native | Leptos/Cytoscape | React/Cytoscape | Vue/Cytoscape estimate | Bevy spatial |
| --- | ---: | ---: | ---: | ---: | ---: |
| Implementation complexity | 3 | 4 | 2 | 2 | 4 |
| Contributor ease | 3 | 4 | 1 | 2 | 4 |
| GRM/Rust code reuse | 1 | 2 | 4 | 4 | 2 |
| Browser deployment fit | 5 | 2 | 1 | 1 | 3 |
| Graph-widget maturity | 4 | 1 | 1 | 1 | 3 |
| Framework maturity | 2 | 3 | 1 | 1 | 3 |
| Build/toolchain friction | 3 | 4 | 2 | 2 | 5 |
| Long-term hiring/ecosystem fit | 4 | 4 | 1 | 2 | 4 |
| Product/admin UI scalability | 3 | 3 | 1 | 1 | 5 |
| Spatial exploration potential | 4 | 3 | 3 | 3 | 1 |
| Risk of semantic creep | 2 | 2 | 2 | 2 | 3 |

## Option Notes

### Native Rust / egui

Strengths:

- Best fit for reusing Rust types and service client code directly.
- Natural for a local developer diagnostic application.
- Keeps frontend language count low.
- `eframe`/`egui` are active and documented; current docs describe native and
  web app support.

Weaknesses:

- The graph visualization layer is thinner. Current `egui_graphs` docs show a
  smaller ecosystem and less complete documentation than mainstream browser
  graph tooling.
- Native distribution becomes its own product concern.
- Browser-hosted cockpit, auth, dashboards, docs links, sharing, and future
  hosted operational views become harder.
- Current spike is pinned behind latest egui/egui_graphs because the repo's
  Rust toolchain cannot compile the newest stack.

Best use:

- Keep as a local developer/diagnostic workbench path only if that need becomes
  explicit.

### Leptos / Rust WASM / Cytoscape

Strengths:

- Preserves Rust frontend implementation.
- Can share Rust snapshot types via `serde` and `serde-wasm-bindgen`.
- Leptos is a capable Rust web framework with SPA, SSR, hydration, and
  full-stack patterns.
- Uses Cytoscape for the graph, which is the strongest part of the browser
  route.

Weaknesses:

- Adds WASM, Trunk, Rust frontend patterns, and JS interop at the same time.
- UI code is compact but less familiar to most web contributors.
- Browser-to-gRPC still needs an HTTP adapter, so Rust frontend does not remove
  the gateway layer.
- Rich UI controls, testing libraries, accessibility workflows, and design
  system integration are likely slower than mainstream TypeScript.

Best use:

- Keep as evidence that Rust/WASM is possible. Do not make it default unless
  Rust-only frontend ownership becomes a strategic requirement.

### React / TypeScript / Cytoscape

Strengths:

- Best contributor familiarity and ecosystem depth.
- Vite gives a simple, fast dev/build loop and first-party React support.
- React has first-class TypeScript documentation, stable release policy, and a
  very large package ecosystem.
- Cytoscape has built-in TypeScript declarations, a mature graph model,
  browser interaction support, layouts, and a large installation base.
- Clear path to dashboards, inspectors, tabs, saved views, explain/profile
  panels, audit-log inspection, and eventually hosted cockpit workflows.

Weaknesses:

- Adds a Node/npm toolchain to a Rust workspace.
- Duplicates snapshot types in TypeScript unless generated types are introduced.
- More verbose than the Leptos spike for the same tiny UI.
- Needs dependency hygiene, lockfile policy, UI testing, and bundle tracking.

Best use:

- Best default for the first real browser cockpit MVP.

### Vue / TypeScript / Cytoscape

Strengths:

- Mature, TypeScript-first browser framework with official Vite scaffolding.
- Often less verbose for form-heavy dashboards than React.
- Strong fit for conventional admin-style UI.

Weaknesses:

- Smaller ecosystem and contributor pool than React, though still very healthy.
- A Vue spike would probably prove similar browser/Cytoscape facts as React.
- It would not remove the same API, gateway, and snapshot-type questions.

Best use:

- Evaluate with a very small spike only if there is a team preference for Vue
  or if React ergonomics become unpleasant in the first real cockpit slice.

### Bevy / Rust Scene Engine

Strengths:

- Best fit for a true spatial memory-map metaphor: camera, depth, clusters,
  paths, landmarks, animation, and eventually richer scene interaction.
- Keeps implementation in Rust and can consume the shared `CockpitSnapshot`
  shape directly.
- Native and `wasm32-unknown-unknown` checks pass in the current spike.
- Strong differentiator for demos if the product wants the cockpit to feel like
  an explorable operational-memory environment rather than only a dashboard.

Weaknesses:

- Highest build/toolchain friction of the spiked paths. Bevy is a large engine
  dependency and the first test build is heavy.
- Current workspace Rust 1.88 resolves Bevy to `0.14.2`; newer Bevy releases
  exist but require newer toolchains. This creates an immediate version-lag
  decision.
- Engine UI is not a natural fit for dense admin controls, tables, filters,
  accessibility, copy/paste workflows, or conventional security/audit panels.
- Web packaging still needs explicit follow-up work even though the WASM check
  passes.

Best use:

- Keep as a parallel spatial explorer track. Do not make it the primary cockpit
  UI until it proves value on representative memory-navigation tasks.

## Bevy Addendum

The Bevy spike usefully changed the shape of the discussion. It showed that a
scene engine can express an explorable memory map, but it also clarified that
this is not the main cockpit problem.

For GRM, the highest-value cockpit workflows are still dense operational
workbench tasks:

- understand available schema;
- inspect graph structure;
- run or replay typed queries;
- inspect explain/profile evidence;
- inspect audit and security context;
- understand writes and durable outcomes.

Those workflows fit a conventional browser application better than a game
engine shell. The Bevy route becomes interesting when the question is spatial
comprehension: clusters, paths, memory landmarks, narrative demonstrations,
time-based replay, or embodied exploration.

The immediate product split should be:

- primary workbench: Vue/Vite or React/Vite, 2D first;
- visualization substrate: typed snapshot plus typed event stream;
- optional spatial explorer: Bevy or another scene engine only after the 2D
  event model exists.

The event-stream layer is the bridge between these worlds. Reads can animate as
traffic over matched nodes and traversed edges; writes can appear as new nodes,
edges, or property changes; explain/profile can show why traffic chose a path;
audit events can show who caused the operation. These events should describe
typed GRM operation observations, not renderer-specific animation commands.

Example event categories:

- query lifecycle: started, planned, matched, completed, failed;
- traversal: node visited, edge traversed, path accepted, path rejected;
- write lifecycle: create requested, accepted, committed, rejected;
- projection changes: node appeared, edge appeared, property changed;
- trust evidence: authorization checked, audit event recorded, actor/principal
  attributed.

This keeps execution animation product-relevant without making animation a
backend contract. The service/runtime remains responsible for typed operations
and evidence; the cockpit chooses how to present those observations.

## Maturity Evidence

External signals checked on 2026-07-27:

- React docs list React 19.2 as the latest major documentation line, and npm
  reports `react` 19.2.7 as latest with very high weekly download volume.
- Vue docs describe first-class TypeScript support, and npm reports Vue 3.5.40
  as latest with high weekly download volume.
- Vite's official guide lists React TypeScript and Vue TypeScript templates,
  built-in TypeScript handling, fast dev server behavior, and current Node
  compatibility expectations.
- Cytoscape.js documentation describes a full graph visualization and analysis
  library with browser gestures, layouts, events, and npm/ESM support; npm
  reports built-in TypeScript declarations.
- Leptos docs describe SPA, SSR, hydration, and progressive enhancement support
  for Rust web apps.
- `eframe` docs describe official native/web egui app support, while
  `egui_graphs` appears active but comparatively smaller and less documented.
- Bevy's official examples and migration material show active web/WASM support,
  but the current latest-compatible version for Rust 1.88 in this workspace is
  Bevy `0.14.2`, not the newest Bevy line. Web checks also require explicit
  `getrandom`/`uuid` JavaScript randomness configuration.

References:

- https://react.dev/versions
- https://react.dev/learn/typescript
- https://www.npmjs.com/package/react
- https://vuejs.org/guide/typescript/overview
- https://www.npmjs.com/package/vue
- https://vite.dev/guide/
- https://vite.dev/guide/features.html
- https://js.cytoscape.org/
- https://www.npmjs.com/package/cytoscape
- https://docs.rs/leptos/latest/leptos/
- https://docs.rs/eframe/latest/eframe/
- https://docs.rs/crate/egui_graphs/latest
- https://bevyengine.org/learn/
- https://bevy.org/examples/webgpu2d/render-to-texture/
- https://bevy.org/learn/migration-guides/0-16-to-0-17/
- https://docs.rs/getrandom/latest/getrandom/

## Additional Comparisons Before Sign-Off

The next comparisons should exercise product reality rather than building more
toy UIs.

1. Representative workspace scale

   Load a real project-memory-sized workspace and a synthetic larger workspace.
   Compare time to first render, layout stability, pan/zoom responsiveness,
   selection latency, memory use, and bundle/runtime size.

2. Explain/profile workflow

   Add one real explain/profile panel over a selected traversal or find result.
   This tests whether the UI stack handles GRM's product-specific trust story,
   not only graph drawing.

3. Schema-first navigation

   Add filters by node model, edge model, and identifying fields. This is core
   cockpit behavior and will expose whether state management remains simple.

4. Generated TypeScript contract

   Decide whether TypeScript snapshot/request types should be hand-written,
   generated from protobuf, generated from JSON schema, or emitted by a Rust
   build helper. This is more important than React versus Vue.

5. UI testability

   Add Playwright coverage for loading a fixture snapshot, selecting a node,
   filtering by model, and verifying that omitted-edge warnings render. This
   should become the browser cockpit regression surface.

6. Dependency and security hygiene

   Record lockfile policy, update cadence, npm audit expectations, and whether
   dev dependencies are allowed in the main repository. If the cockpit becomes
   distributable, add bundle-size and license checks.

7. Deployment shape

   Compare local-only dev mode, Docker-served static assets, and a future hosted
   reverse-proxy shape. The cockpit should still talk to a typed GRM adapter,
   not directly to arbitrary backend internals.

8. Accessibility and dense-workbench UX

   Test keyboard focus, contrast, resizable panes, inspector scrolling,
   model-list scanning, and mobile behavior. The cockpit is an operational tool,
   so dense and predictable beats decorative.

9. Spatial memory explorer value

   Use the Bevy spike against a real project-memory snapshot and ask whether
   depth, camera movement, clusters, and path highlighting reveal relationships
   that are hard to see in 2D. If the answer is only "it looks interesting",
   keep it as demo/research rather than product cockpit.

## Proposed Implementation Sign-Off Gate

Sign off on the first implementation inside the accepted Vue/Vite or React/Vite
direction when:

- The selected spike can load a representative workspace through the existing
  gateway.
- One schema filter and one selection inspector workflow are covered by
  Playwright.
- A basic explain/profile panel can be added without awkward state management.
- TypeScript contract generation or hand-maintained type ownership is decided.
- Bundle size and dependency policy are recorded.
- The document is either promoted into an ADR or linked from the cockpit roadmap
  item as accepted direction.

The Bevy working direction remains:

> A parked spatial memory explorer experiment over the same bounded snapshot and
> future event-stream model, evaluated only after the 2D cockpit proves the core
> operational workflows.

## Non-Goals For The Next Slice

- No graph mutation.
- No schema editing.
- No arbitrary command console.
- No direct browser gRPC.
- No hosted security or admin claims.
- No policy authoring.
- No replacement of the typed gRPC/protobuf workspace boundary.
