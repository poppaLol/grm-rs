# ADR 0012: Use Visual Projection Schema While Keeping The Flight-Deck Web-First

Status: Accepted

Date: 2026-09-16

## Context

GRM represents operational memory as graph data shaped by runtime schema.
Earlier decisions separated graph data from schema memory, treated the workspace
catalog as schema authority, and kept the flight-deck as a 2D-first TypeScript
browser workbench over typed GRM service/runtime operations.

The next product opportunity is more than rendering a schema diagram. The data
is graph-shaped, the schema is graph-shaped, and the schema metadata can also
be understood as a small graph of models, links, fields, and properties. That
opens a further layer: a typed visual projection over graph data and schema
metadata.

This raises a product boundary question. A graph workbench with visual
projection, selection, layout, local graph state, and future execution animation
could start to feel like a rich client application rather than a conventional
web app. GRM needs that richness without accidentally creating a second
canonical runtime or moving product semantics into UI state.

## Decision

GRM will introduce visual projection schema as a typed projection layer over
graph data, runtime schema, and schema metadata.

The flight-deck remains web-first for the current product path: a TypeScript
browser workbench, backed by typed service/runtime responses and a local
GRM-shaped browser store. It may become richer in interaction, state, and
layout, but browser state remains a cache and projection rather than canonical
GRM memory.

Visual projection schema describes how graph and schema entities should be
presented. It may include:

- default node glyphs, colors, labels, and grouping hints;
- default edge styles, labels, and emphasis rules;
- schema-model and link summaries for detail panels;
- layout hints and collapse/expand affordances;
- default projections for graph data, schema graphs, and schema-meta graphs;
- later user-customized projection overlays.

The first implementation direction should be generated default projections,
exposed through an MCP/helper or service/runtime surface, before user-authored
visual schema editing exists.

Visual projection is not the canonical GRM service contract. Canonical GRM
behavior remains typed structured operations over graph workspaces. Visual
projection schema is a structured adapter/projection that helps humans and
agents understand, navigate, and render the graph.

## Web App Versus Rich Client Boundary

GRM should not choose a native rich client as the default direction yet.

The current evidence supports a web-first workbench because:

- the accepted flight-deck direction is 2D-first TypeScript browser UI;
- the local graph store already preserves a projection/cache boundary;
- service-backed typed operations remain the stable integration point;
- browser delivery keeps the first human-facing surface easy to try and ship;
- richer native or WASM-backed behavior can sit behind the same projection
  contract later.

GRM should revisit a native or sidecar-heavy rich client if concrete needs make
browser delivery the limiting factor. Examples include very large local
workspaces requiring specialized local storage, offline-first editing with real
runtime validation, OS-integrated secure credential handling, GPU-heavy
rendering, or local workspace orchestration that cannot safely be represented
through the existing browser/gateway/service model.

## Non-Goals

- No native desktop application commitment.
- No browser-side canonical GRM runtime.
- No user-authored visual schema editor in the first slice.
- No 3D/spatial navigation requirement.
- No new textual query language or visual programming language.
- No claim that visual layout, color, or glyph choice changes graph semantics.
- No persistence of secrets, private keys, raw certificates, or server-local
  paths in browser state.

## Consequences

Positive consequences:

- GRM can provide a useful default graph/schema visualization without waiting
  for a full visual editor.
- The flight-deck can become a richer workbench while preserving typed service
  and workspace authority.
- MCP agents can ask for visual projection hints instead of inventing one-off
  graph renderings.
- Future custom visual schemas have a product home that is distinct from graph
  data and schema authority.
- A later WASM, sidecar, or native client can reuse the same visual projection
  concepts rather than forking GRM semantics.

Tradeoffs:

- GRM gains another projected schema-like layer that needs clear naming and
  docs.
- Projection defaults must avoid implying semantic authority they do not have.
- The flight-deck will need tests that prove visual projection is applied as
  UI state over typed service/runtime truth.

## Required Proof For The First Slice

The first slice should prove a default generated visual projection without a
full editor:

- define a small typed visual projection shape;
- generate default projection data from runtime schema metadata;
- expose it through an MCP/helper or service/runtime adapter surface;
- render or serialize enough of the projection for the flight-deck or an agent
  to produce a sensible first graph/schema view;
- test the public surface that returns the projection;
- document that projection is advisory UI/agent guidance, not canonical graph
  semantics.

## Open Questions

- Should visual projection live in the service API, MCP helper surface, runtime
  helper, or all three through a shared core shape?
- How should user-customized visual projection overlays be stored: workspace
  catalog metadata, user profile, separate UI workspace state, or exportable
  view definitions?
- What is the minimum projection shape that works for both schema graphs and
  user data graphs?
- When does the flight-deck need WASM or sidecar-backed local runtime behavior,
  if ever?
