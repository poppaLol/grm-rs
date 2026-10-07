import type { FlightDeckSecurityAuditStatus, FlightDeckSecurityStatus, FlightDeckSnapshot, FlightDeckVisualProjection } from "./types";

const roadmapItems = [
  { id: "13", title: "Build the GRM flight-deck", status: "active", summary: "Give developers a visual workbench for inspecting software delivery memory.", rank: 1 },
  { id: "14", title: "Deliver a secure typed API", status: "active", summary: "Apply identity, workspace permissions, and audit to structured service operations.", rank: 2 },
  { id: "15", title: "Make workspace memory durable", status: "active", summary: "Recover typed project memory reliably after restarts and failed writes.", rank: 3 },
  { id: "16", title: "Connect developer tools and agents", status: "planned", summary: "Give CLI, MCP, and Python clients the same typed operation semantics.", rank: 4 },
  { id: "17", title: "Ship a repeatable release pipeline", status: "planned", summary: "Build, test, package, and verify a release that another team can reproduce.", rank: 5 }
];

const workSlices = [
  { id: "566", roadmap: "13", title: "Promote React/Vite flight-deck first service UI", status: "completed", readiness: 5, summary: "The first workbench can load a bounded snapshot from the typed service.", requirements: ["315", "316"], controls: ["337"] },
  { id: "567", roadmap: "13", title: "Inspect schema models and legal link directions", status: "completed", readiness: 5, summary: "Render model fields and relationship endpoints separately from data instances.", requirements: ["315", "318"], controls: ["337", "340"] },
  { id: "568", roadmap: "13", title: "Design graph regions and local visual overlays", status: "active", readiness: 4, summary: "Explore labels, colours, shapes, and collapsible regions without changing workspace facts.", requirements: ["316", "318"], controls: ["338", "340"] },
  { id: "569", roadmap: "13", title: "Review workbench with a software team", status: "planned", readiness: 3, summary: "Use a realistic delivery workspace to review navigation, filtering, and selection.", requirements: ["316"], controls: ["338"] },
  { id: "570", roadmap: "14", title: "Share typed request validation across adapters", status: "completed", readiness: 5, summary: "Reject unsupported operations consistently before they reach the backend.", requirements: ["315"], controls: ["337"] },
  { id: "571", roadmap: "14", title: "Map client identities to service principals", status: "active", readiness: 4, summary: "Bind authenticated client identity to the principal evaluated by service policy.", requirements: ["317"], controls: ["339"] },
  { id: "572", roadmap: "14", title: "Enforce workspace permissions on every request", status: "blocked", readiness: 2, summary: "Blocked until the team agrees which agent roles may read and write each workspace.", requirements: ["315", "317", "316"], controls: ["337", "339"] },
  { id: "573", roadmap: "14", title: "Expose bounded read-only audit evidence", status: "planned", readiness: 3, summary: "Make operation outcomes inspectable without exposing graph values or credentials.", requirements: ["316", "319"], controls: ["338", "341"] },
  { id: "574", roadmap: "15", title: "Persist workspace schema with graph data", status: "completed", readiness: 5, summary: "Keep model definitions available when a workspace is reopened.", requirements: ["318", "320"], controls: ["340", "342"] },
  { id: "575", roadmap: "15", title: "Verify restart recovery after interrupted writes", status: "active", readiness: 4, summary: "Exercise restart recovery using isolated test workspaces and failure injection.", requirements: ["320"], controls: ["342"] },
  { id: "576", roadmap: "15", title: "Rehearse backup and restore for a team workspace", status: "blocked", readiness: 2, summary: "Blocked pending an agreed recovery target and a verified backup retention policy.", requirements: ["316", "320"], controls: ["338", "342"] },
  { id: "577", roadmap: "15", title: "Document workspace migration compatibility", status: "planned", readiness: 3, summary: "Record supported schema changes and prove that older clients fail clearly.", requirements: ["318", "321"], controls: ["340", "343"] },
  { id: "578", roadmap: "16", title: "Align CLI operations with the runtime contract", status: "completed", readiness: 5, summary: "Keep text parsing in the CLI while passing typed requests into the runtime.", requirements: ["315", "318"], controls: ["337", "340"] },
  { id: "579", roadmap: "16", title: "Add bounded MCP project-memory inspection", status: "active", readiness: 4, summary: "Let agents inspect declared models and relationships through structured tools.", requirements: ["315", "316", "317"], controls: ["337", "338", "339"] },
  { id: "580", roadmap: "16", title: "Verify Python client compatibility", status: "planned", readiness: 3, summary: "Prove the Python adapter returns the same typed results as the service client.", requirements: ["315", "321"], controls: ["337", "343"] },
  { id: "581", roadmap: "16", title: "Review agent onboarding and error recovery", status: "planned", readiness: 3, summary: "Practice schema discovery, narrow reads, and recovery from invalid requests.", requirements: ["316", "318"], controls: ["338", "340"] },
  { id: "582", roadmap: "17", title: "Run public-boundary checks in CI", status: "completed", readiness: 5, summary: "Test runtime, adapter, and browser behaviour at the boundary that owns it.", requirements: ["315", "321"], controls: ["337", "343"] },
  { id: "583", roadmap: "17", title: "Package versioned UI and gateway artifacts", status: "active", readiness: 4, summary: "Build the workbench and gateway from the same reviewed source revision.", requirements: ["318", "321"], controls: ["340", "343"] },
  { id: "584", roadmap: "17", title: "Approve release evidence and security review", status: "blocked", readiness: 2, summary: "Blocked until the release owner reviews audit evidence and open security findings.", requirements: ["317", "319", "321"], controls: ["339", "341", "343"] },
  { id: "585", roadmap: "17", title: "Rehearse deployment and rollback", status: "planned", readiness: 3, summary: "Exercise the release procedure against a disposable staging workspace.", requirements: ["320", "321"], controls: ["342", "343"] }
];

const securityRequirements = [
  { id: "315", title: "Use One Canonical Enforcement Pipeline", status: "partial", priority: "required", summary: "Adapters must share runtime validation and service enforcement." },
  { id: "316", title: "Keep inspection bounded and credentials private", status: "active", priority: "required", summary: "Inspection limits and safe browser configuration protect operational memory." },
  { id: "317", title: "Authorize operations with explicit identity and scope", status: "partial", priority: "required", summary: "Policy evaluates the authenticated principal, workspace, and requested operation." },
  { id: "318", title: "Keep typed schema authoritative", status: "active", priority: "required", summary: "Models, fields, and legal edge directions remain workspace facts, separate from visual style." },
  { id: "319", title: "Make service outcomes auditable", status: "planned", priority: "required", summary: "Service-authored audit records show what was attempted and its outcome." },
  { id: "320", title: "Recover durable workspace memory predictably", status: "partial", priority: "required", summary: "Restart and restore must preserve accepted data and schema." },
  { id: "321", title: "Prove compatibility before release", status: "planned", priority: "recommended", summary: "Public-boundary tests and reproducible artifacts support a reviewable release." }
];

const securityControls = [
  { id: "337", title: "Canonical Service Enforcement Pipeline", status: "partial", layer: "service", controlType: "preventive", summary: "Apply shared validation and authorization before backend execution." },
  { id: "338", title: "Bounded Read-Only Inspection Gateway", status: "active", layer: "adapter", controlType: "preventive", summary: "Expose a bounded graph view and keep credential material outside the browser." },
  { id: "339", title: "Principal and Workspace Permission Checks", status: "partial", layer: "service", controlType: "preventive", summary: "Evaluate caller identity and workspace scope for each operation." },
  { id: "340", title: "Runtime Schema Validation", status: "active", layer: "runtime", controlType: "preventive", summary: "Validate node fields and relationship endpoints against declared models." },
  { id: "341", title: "Bounded Service Audit Sink", status: "planned", layer: "service", controlType: "detective", summary: "Retain bounded evidence of service request outcomes." },
  { id: "342", title: "Isolated Recovery and Restore Checks", status: "partial", layer: "storage", controlType: "recovery", summary: "Exercise interrupted writes and restore without touching shared working memory." },
  { id: "343", title: "Public Contract and Release Checks", status: "active", layer: "delivery", controlType: "detective", summary: "Verify runtime and adapter compatibility through their public surfaces." }
];

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
      label: "Software delivery SOML",
      props: { productContextId: 1, name: "Software delivery SOML", status: "active", summary: "Fictional development programme for exploring typed operational memory, visual grouping, and shared delivery constraints." }
    },
    ...roadmapItems.map(({ id, ...props }) => ({ id, model: "RoadmapItem", label: props.title, props: { roadmapItemId: Number(id), ...props } })),
    ...workSlices.map(({ id, roadmap: _roadmap, requirements: _requirements, controls: _controls, ...props }) => ({ id, model: "WorkSlice", label: props.title, props: { workSliceId: Number(id), ...props } })),
    ...securityRequirements.map(({ id, ...props }) => ({ id, model: "SecurityRequirement", label: props.title, props: { securityRequirementId: Number(id), ...props } })),
    ...securityControls.map(({ id, ...props }) => ({ id, model: "SecurityControl", label: props.title, props: { securityControlId: Number(id), ...props } }))
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
    },
    ...roadmapItems.filter((item) => item.id !== "13").map((item) => ({ id: `roadmap:${item.id}`, model: "HAS_ROADMAP_ITEM", from: "1", to: item.id, props: { reason: item.summary } })),
    ...workSlices.filter((slice) => slice.id !== "566").map((slice) => ({ id: `work:${slice.id}`, model: "HAS_WORK_SLICE", from: slice.roadmap, to: slice.id, props: { reason: "Delivery slice within this roadmap area." } })),
    ...workSlices.flatMap((slice) => slice.requirements.filter((id) => slice.id !== "566" || id !== "315").map((id) => ({ id: `requirement:${slice.id}:${id}`, model: "SLICE_ADDRESSES_SECURITY_REQUIREMENT", from: slice.id, to: id, props: { reason: "This slice must satisfy the shared requirement." } }))),
    ...workSlices.flatMap((slice) => slice.controls.filter((id) => slice.id !== "566" || id !== "337").map((id) => ({ id: `control:${slice.id}:${id}`, model: "SLICE_TARGETS_SECURITY_CONTROL", from: slice.id, to: id, props: { reason: "This slice implements or verifies the shared control." } })))
  ],
  modelLimit: 50,
  omittedEdges: 0,
  source: "fixture",
  partialReason: "Fictional software-delivery fixture; statuses and security controls are illustrative, not live project or service evidence."
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
