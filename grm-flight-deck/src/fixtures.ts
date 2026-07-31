import type { FlightDeckSnapshot } from "./types";

export const fixtureSnapshot: FlightDeckSnapshot = {
  workspace: "flight-deck-demo",
  nodeModels: [
    "ProductContext",
    "RoadmapItem",
    "WorkSlice",
    "SecurityRequirement",
    "SecurityControl"
  ],
  edgeModels: [
    "HAS_ROADMAP_ITEM",
    "HAS_WORK_SLICE",
    "SLICE_ADDRESSES_SECURITY_REQUIREMENT",
    "SLICE_TARGETS_SECURITY_CONTROL"
  ],
  schemaEdges: [
    {
      model: "HAS_ROADMAP_ITEM",
      fromModel: "ProductContext",
      toModel: "RoadmapItem"
    },
    {
      model: "HAS_WORK_SLICE",
      fromModel: "RoadmapItem",
      toModel: "WorkSlice"
    },
    {
      model: "SLICE_ADDRESSES_SECURITY_REQUIREMENT",
      fromModel: "WorkSlice",
      toModel: "SecurityRequirement"
    },
    {
      model: "SLICE_TARGETS_SECURITY_CONTROL",
      fromModel: "WorkSlice",
      toModel: "SecurityControl"
    }
  ],
  nodes: [
    {
      id: "1",
      model: "ProductContext",
      label: "GRM project memory",
      props: { status: "active" }
    },
    {
      id: "13",
      model: "RoadmapItem",
      label: "Build the GRM flight-deck",
      props: { status: "planned", rank: 4 }
    },
    {
      id: "566",
      model: "WorkSlice",
      label: "Promote React/Vite flight-deck first service UI",
      props: { status: "planned", readiness: 3 }
    },
    {
      id: "315",
      model: "SecurityRequirement",
      label: "Use One Canonical Enforcement Pipeline",
      props: { status: "partial", priority: "required" }
    },
    {
      id: "337",
      model: "SecurityControl",
      label: "Canonical Service Enforcement Pipeline",
      props: { status: "partial", layer: "service" }
    }
  ],
  edges: [
    {
      id: "1",
      model: "HAS_ROADMAP_ITEM",
      from: "1",
      to: "13",
      props: { reason: "flight-deck roadmap context" }
    },
    {
      id: "2",
      model: "HAS_WORK_SLICE",
      from: "13",
      to: "566",
      props: { reason: "first service UI slice" }
    },
    {
      id: "3",
      model: "SLICE_ADDRESSES_SECURITY_REQUIREMENT",
      from: "566",
      to: "315",
      props: { reason: "UI remains an adapter over typed operations" }
    },
    {
      id: "4",
      model: "SLICE_TARGETS_SECURITY_CONTROL",
      from: "566",
      to: "337",
      props: { reason: "future service calls enter canonical enforcement" }
    }
  ],
  modelLimit: 50,
  omittedEdges: 0,
  source: "fixture",
  partialReason: "Fixture data is bundled for local UI development before a service adapter is running."
};
