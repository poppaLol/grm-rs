import type { FlightDeckSecurityAuditStatus, FlightDeckSecurityStatus, FlightDeckSnapshot, FlightDeckVisualProjection } from "./types";

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


export const fixtureVisualProjection: FlightDeckVisualProjection = {
  workspace: "flight-deck-demo",
  provenance: {
    source: "fixture_default",
    generatedFrom: "local_fixture_schema_metadata",
    advisory: true,
    modelLimit: 50
  },
  nodeModels: [
    {
      model: "ProductContext",
      label: "product context",
      glyph: "node",
      colorToken: "flight-deck-teal",
      group: "node:ProductContext",
      idField: "productContextId",
      detailFields: ["productContextId", "name", "summary", "status"]
    },
    {
      model: "RoadmapItem",
      label: "roadmap item",
      glyph: "plan",
      colorToken: "flight-deck-blue",
      group: "node:RoadmapItem",
      idField: "roadmapItemId",
      detailFields: ["roadmapItemId", "title", "summary", "status", "rank"]
    },
    {
      model: "WorkSlice",
      label: "work slice",
      glyph: "plan",
      colorToken: "flight-deck-indigo",
      group: "node:WorkSlice",
      idField: "workSliceId",
      detailFields: ["workSliceId", "title", "summary", "status", "readiness"]
    },
    {
      model: "SecurityRequirement",
      label: "security requirement",
      glyph: "shield",
      colorToken: "flight-deck-violet",
      group: "node:SecurityRequirement",
      idField: "securityRequirementId",
      detailFields: ["securityRequirementId", "title", "summary", "priority", "status"]
    },
    {
      model: "SecurityControl",
      label: "security control",
      glyph: "shield",
      colorToken: "flight-deck-rose",
      group: "node:SecurityControl",
      idField: "securityControlId",
      detailFields: ["securityControlId", "title", "summary", "controlType", "layer", "status"]
    }
  ],
  edgeModels: [
    {
      model: "HAS_ROADMAP_ITEM",
      label: "has roadmap item",
      styleToken: "directed",
      directionEmphasis: "directed",
      group: "edge:ProductContext->RoadmapItem",
      fromModel: "ProductContext",
      toModel: "RoadmapItem",
      detailFields: ["hasRoadmapItemId", "reason"]
    },
    {
      model: "HAS_WORK_SLICE",
      label: "has work slice",
      styleToken: "directed",
      directionEmphasis: "directed",
      group: "edge:RoadmapItem->WorkSlice",
      fromModel: "RoadmapItem",
      toModel: "WorkSlice",
      detailFields: ["hasWorkSliceId", "reason"]
    },
    {
      model: "SLICE_ADDRESSES_SECURITY_REQUIREMENT",
      label: "slice addresses security requirement",
      styleToken: "directed",
      directionEmphasis: "directed",
      group: "edge:WorkSlice->SecurityRequirement",
      fromModel: "WorkSlice",
      toModel: "SecurityRequirement",
      detailFields: ["sliceAddressesSecurityRequirementId", "reason"]
    },
    {
      model: "SLICE_TARGETS_SECURITY_CONTROL",
      label: "slice targets security control",
      styleToken: "directed",
      directionEmphasis: "directed",
      group: "edge:WorkSlice->SecurityControl",
      fromModel: "WorkSlice",
      toModel: "SecurityControl",
      detailFields: ["sliceTargetsSecurityControlId", "reason"]
    }
  ],
  schemaLayout: {
    layoutToken: "schema-by-endpoints",
    groupBy: "node-model,edge-model,relationship-endpoints",
    rankBy: "model-label"
  },
  dataLayout: {
    layoutToken: "data-by-model",
    groupBy: "model",
    rankBy: "model-label"
  }
};

export const fixtureSecurityStatus: FlightDeckSecurityStatus = {
  securityProfile: "fixture",
  identityStatus: "fixture",
  principal: null,
  authenticationMethod: null,
  policyVersion: null
};

export const fixtureSecurityAuditStatus: FlightDeckSecurityAuditStatus = {
  securityProfile: "fixture",
  auditMode: "not_applicable",
  sinkHealth: "unknown",
  mandatoryAuditAvailable: false,
  retainedEventCount: 0,
  recentEventCount: 0,
  retentionMaxEvents: 0,
  retentionMaxBytes: 0,
  retentionMaxAgeSeconds: 0,
  futureDatedRecordCount: 0,
  lastRecoveryStatusCode: null,
  recentEvents: []
};
