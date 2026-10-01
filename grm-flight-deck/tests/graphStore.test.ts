import assert from "node:assert/strict";
import test from "node:test";

import {
  createFlightDeckGraphStore,
  LEGACY_CONNECTION_STORAGE_KEY,
  normalizeSnapshot,
  STORE_STORAGE_KEY
} from "../src/graphStore";
import {
  applyVisualProjectionOverlay,
  emptyVisualProjectionOverlay,
  readVisualProjectionOverlay,
  VISUAL_OVERLAY_STORAGE_KEY,
  writeVisualProjectionOverlay
} from "../src/projectionOverlay";
import type { FlightDeckSnapshot, FlightDeckVisualProjection } from "../src/types";

class MemoryStorage {
  private values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
}

const snapshot: FlightDeckSnapshot = {
  workspace: "flight-deck-demo",
  nodeModels: ["RoadmapItem", "WorkSlice"],
  edgeModels: ["HAS_WORK_SLICE"],
  schemaEdges: [{ model: "HAS_WORK_SLICE", fromModel: "RoadmapItem", toModel: "WorkSlice" }],
  nodes: [
    {
      id: "13",
      model: "RoadmapItem",
      label: "Build the GRM flight-deck",
      props: { status: "active" }
    },
    {
      id: "573",
      model: "WorkSlice",
      label: "Implement flight-deck local graph store and connection profiles",
      props: { status: "planned" }
    }
  ],
  edges: [
    {
      id: "1050",
      model: "HAS_WORK_SLICE",
      from: "13",
      to: "573",
      props: { reason: "next planned slice" }
    }
  ],
  modelLimit: 50,
  omittedEdges: 0,
  source: "fixture"
};

const visualProjection: FlightDeckVisualProjection = {
  workspace: "flight-deck-demo",
  provenance: {
    source: "generated_default",
    generatedFrom: "runtime_schema_metadata",
    advisory: true,
    modelLimit: 50
  },
  nodeModels: [
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
      detailFields: ["workSliceId", "title", "summary", "status"]
    }
  ],
  edgeModels: [
    {
      model: "HAS_WORK_SLICE",
      label: "has work slice",
      styleToken: "directed",
      directionEmphasis: "directed",
      group: "edge:RoadmapItem->WorkSlice",
      fromModel: "RoadmapItem",
      toModel: "WorkSlice",
      detailFields: ["hasWorkSliceId", "reason"]
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

test("starts with a production-sized default model limit and clamps large requests", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());

  assert.equal(store.getState().settings.limit, 5000);
  store.updateSettings({ limit: 150000 });
  assert.equal(store.getState().settings.limit, 25000);
});

test("migrates the legacy single connection settings key into a safe profile", () => {
  const storage = new MemoryStorage();
  storage.setItem(
    LEGACY_CONNECTION_STORAGE_KEY,
    JSON.stringify({
      serviceBaseUrl: "http://user:password@127.0.0.1:3001?token=abc#secret",
      workspace: "restored-workspace",
      limit: 75,
      useFixtureData: false,
      privateKeyPath: "/tmp/client.key",
      bearerToken: "secret"
    })
  );

  const store = createFlightDeckGraphStore(storage);
  const state = store.getState();

  assert.equal(state.profiles.length, 1);
  assert.equal(state.settings.workspace, "restored-workspace");
  assert.equal(state.settings.limit, 75);
  assert.equal(state.settings.serviceBaseUrl, "http://127.0.0.1:3001");
  assert.equal("privateKeyPath" in state.profiles[0].settings, false);
  assert.equal("bearerToken" in state.profiles[0].settings, false);
});

test("persists selected profile settings with only allowlisted fields", () => {
  const storage = new MemoryStorage();
  const store = createFlightDeckGraphStore(storage);

  store.updateSettings({
    serviceBaseUrl: "http://127.0.0.1:3001",
    workspace: "project-memory",
    limit: 200,
    useFixtureData: false
  });
  store.updateDraftProfileName("Secured local");
  store.saveCurrentProfile();

  const persisted = storage.getItem(STORE_STORAGE_KEY);
  assert.ok(persisted);
  assert.deepEqual(Object.keys(JSON.parse(persisted).profiles[0].settings).sort(), [
    "limit",
    "mode",
    "serviceBaseUrl",
    "useFixtureData",
    "workspace"
  ]);

  const restored = createFlightDeckGraphStore(storage).getState();
  assert.equal(restored.selectedProfileId, "local-workspace");
  assert.equal(restored.settings.workspace, "project-memory");
});

test("scrubs credential-bearing service URLs before browser persistence", () => {
  const storage = new MemoryStorage();
  const store = createFlightDeckGraphStore(storage);

  store.updateSettings({
    serviceBaseUrl: "http://admin:secret@127.0.0.1:3001/grm?token=abc#client-key",
    useFixtureData: false
  });
  store.saveCurrentProfile();

  const persisted = storage.getItem(STORE_STORAGE_KEY);
  assert.ok(persisted);
  const persistedUrl = JSON.parse(persisted).profiles[0].settings.serviceBaseUrl;

  assert.equal(persistedUrl, "http://127.0.0.1:3001/grm");
  assert.equal(persistedUrl.includes("admin"), false);
  assert.equal(persistedUrl.includes("secret"), false);
  assert.equal(persistedUrl.includes("token"), false);

  store.updateSettings({
    serviceBaseUrl: "https://admin:secret@127.0.0.1:3001/client.key?token=abc#fingerprint"
  });
  store.saveCurrentProfile();
  const secretPathUrl = JSON.parse(storage.getItem(STORE_STORAGE_KEY) ?? "{}").profiles[0].settings.serviceBaseUrl;

  assert.equal(secretPathUrl, "https://127.0.0.1:3001");
  assert.equal(secretPathUrl.includes("client.key"), false);
  assert.equal(secretPathUrl.includes("fingerprint"), false);
});

test("can create a second selected connection profile from draft settings", () => {
  const storage = new MemoryStorage();
  const store = createFlightDeckGraphStore(storage);

  store.updateSettings({ workspace: "first-workspace" });
  store.saveCurrentProfile();
  store.updateDraftProfileName("Second profile");
  store.updateSettings({ workspace: "second-workspace", useFixtureData: false });
  store.createProfile();

  const state = store.getState();
  assert.equal(state.profiles.length, 2);
  assert.equal(state.selectedProfileId, "second-profile");
  assert.equal(state.settings.workspace, "second-workspace");

  store.selectProfile("local-workspace");
  assert.equal(store.getState().settings.workspace, "first-workspace");
});

test("applies browser-local visual projection overlays without changing generated defaults", () => {
  const overlay = emptyVisualProjectionOverlay("local-workspace", "flight-deck-demo");
  overlay.nodeModels.RoadmapItem = {
    label: "PM roadmap",
    colorToken: "flight-deck-amber",
    group: "product planning",
    shape: "card",
    detailDensity: "rich",
    visualRole: "anchor"
  };
  overlay.edgeModels.HAS_WORK_SLICE = {
    styleToken: "dependency",
    directionEmphasis: "strong",
    lineWeight: "strong",
    lineStyle: "dashed",
    labelVisibility: "always"
  };
  overlay.containers["edge:HAS_WORK_SLICE"] = {
    parentModel: "RoadmapItem",
    childModel: "WorkSlice",
    viaEdgeModel: "HAS_WORK_SLICE",
    renderAs: "section",
    collapse: "expanded"
  };
  overlay.layout = { mode: "container-map", style: "groups" };

  const projected = applyVisualProjectionOverlay(visualProjection, overlay);

  assert.equal(projected?.nodeModels[0].label, "PM roadmap");
  assert.equal(projected?.nodeModels[0].colorToken, "flight-deck-amber");
  assert.equal(projected?.nodeModels[0].group, "product planning");
  assert.equal(projected?.nodeModels[0].shape, "card");
  assert.equal(projected?.nodeModels[0].detailDensity, "rich");
  assert.equal(projected?.nodeModels[0].visualRole, "anchor");
  assert.equal(projected?.edgeModels[0].styleToken, "dependency");
  assert.equal(projected?.edgeModels[0].labelVisibility, "always");
  assert.equal(projected?.dataLayout.layoutToken, "container-map-local");
  assert.equal(overlay.containers["edge:HAS_WORK_SLICE"].renderAs, "section");
  assert.equal(visualProjection.nodeModels[0].label, "roadmap item");
});

test("persists local visual projection overlays by profile and workspace only", () => {
  const storage = new MemoryStorage();
  const overlay = emptyVisualProjectionOverlay("owner-profile", "sygnal-one-memory");
  overlay.nodeModels.WorkSlice = {
    label: "delivery slice",
    colorToken: "flight-deck-green",
    group: "roadmap",
    visualRole: "process"
  };
  overlay.edgeModels.HAS_WORK_SLICE = { styleToken: "evidence", labelVisibility: "always" };
  overlay.containers["edge:HAS_WORK_SLICE"] = {
    parentModel: "RoadmapItem",
    childModel: "WorkSlice",
    viaEdgeModel: "HAS_WORK_SLICE",
    renderAs: "lane"
  };
  overlay.layout = { mode: "container-map" };

  writeVisualProjectionOverlay(storage, overlay);

  const restored = readVisualProjectionOverlay(storage, "owner-profile", "sygnal-one-memory");
  const otherWorkspace = readVisualProjectionOverlay(storage, "owner-profile", "other-workspace");
  const persisted = storage.getItem(VISUAL_OVERLAY_STORAGE_KEY) ?? "";

  assert.equal(restored.nodeModels.WorkSlice.label, "delivery slice");
  assert.equal(restored.nodeModels.WorkSlice.visualRole, "process");
  assert.equal(restored.edgeModels.HAS_WORK_SLICE.styleToken, "evidence");
  assert.equal(restored.containers["edge:HAS_WORK_SLICE"].renderAs, "lane");
  assert.equal(restored.layout.mode, "container-map");
  assert.deepEqual(otherWorkspace.nodeModels, {});
  assert.equal(persisted.includes("BEGIN CERTIFICATE"), false);
  assert.equal(persisted.includes("privateKey"), false);
});

test("keeps visual projection as non-persistent workbench state", () => {
  const storage = new MemoryStorage();
  const store = createFlightDeckGraphStore(storage);

  store.loadVisualProjection(visualProjection);
  store.saveCurrentProfile();

  assert.equal(store.getState().visualProjection?.provenance.advisory, true);
  assert.equal(store.getState().visualProjection?.nodeModels[0].detailFields[0], "roadmapItemId");
  assert.equal(storage.getItem(STORE_STORAGE_KEY)?.includes("visualProjection"), false);

  store.markConnectionFailed("service unavailable");
  assert.equal(store.getState().visualProjection, null);
});

test("normalizes snapshots by stable string node and edge ids", () => {
  const normalized = normalizeSnapshot(snapshot);

  assert.equal(normalized.nodesById["13"].label, "Build the GRM flight-deck");
  assert.equal(normalized.edgesById["1050"].to, "573");
  assert.deepEqual(normalized.nodeIds, ["13", "573"]);
  assert.deepEqual(normalized.edgeIds, ["1050"]);
});

test("derives visible graph from filter state and clears disappeared selection", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);
  store.selectGraphItem({ kind: "node", id: "13" });
  assert.equal(store.getState().selectedItem?.label, "Build the GRM flight-deck");

  store.applyGraphFilter({
    text: "connection profiles",
    model: "",
    propertyKey: "",
    propertyValue: ""
  });

  const state = store.getState();
  assert.deepEqual(state.visibleSnapshot?.nodes.map((node) => node.id), ["573"]);
  assert.equal(state.visibleSnapshot?.edges.length, 0);
  assert.equal(state.selectedItem, null);
  assert.equal(state.selection, null);
});

test("clears graph selection when switching workspace view context", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);

  store.selectGraphItem({ kind: "node", id: "13" });
  assert.equal(store.getState().selectedItem?.id, "13");

  store.setGraphView("schema");
  assert.equal(store.getState().selection, null);
  assert.equal(store.getState().selectedItem, null);

  store.selectGraphItem({ kind: "node", id: "schema-node:RoadmapItem" });
  assert.equal(store.getState().selectedItem?.id, "schema-node:RoadmapItem");

  store.selectWorkspacePanel("audit");
  assert.equal(store.getState().selection, null);
  assert.equal(store.getState().selectedItem, null);
});

test("starts in the query panel with connection details and insights hidden", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  const state = store.getState();

  assert.equal(state.activeWorkspacePanel, "query");
  assert.equal(state.graphView, "data");
  assert.match(state.queryCommand, /^node\.find/);
  assert.equal(state.queryStatus, "idle");
  assert.equal(state.queryEvidence, null);
  assert.equal(state.connectionDetailsOpen, false);
  assert.equal(state.explainVisible, false);
  assert.equal(state.profileVisible, false);

  store.selectWorkspacePanel("audit");
  store.setGraphView("schema");
  store.setConnectionDetailsOpen(true);
  store.setExplainVisible(true);
  store.setProfileVisible(true);

  const changed = store.getState();
  assert.equal(changed.activeWorkspacePanel, "audit");
  assert.equal(changed.graphView, "schema");
  assert.equal(changed.connectionDetailsOpen, true);
  assert.equal(changed.explainVisible, true);
  assert.equal(changed.profileVisible, true);
});

test("tracks command text without persisting command history", () => {
  const storage = new MemoryStorage();
  const store = createFlightDeckGraphStore(storage);

  store.setQueryCommand("node.find WorkSlice secret_token=abc limit=5");
  store.saveCurrentProfile();

  assert.equal(store.getState().queryCommand, "node.find WorkSlice secret_token=abc limit=5");
  const persisted = storage.getItem(STORE_STORAGE_KEY);
  assert.ok(persisted);
  assert.equal(persisted.includes("secret_token"), false);
  assert.equal(persisted.includes("queryCommand"), false);
});

test("applies service query responses as visible graph state with service evidence", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);
  store.applyGraphFilter({
    text: "connection profiles",
    model: "",
    propertyKey: "",
    propertyValue: ""
  });

  store.applyQueryResponse({
    workspace: "flight-deck-demo",
    command: "session.explain node.find WorkSlice status=active limit=25",
    kind: "explain",
    queryShape: "node.find",
    evidence: {
      provenance: "service",
      label: "service/runtime explain evidence",
      planKind: "node.find",
      steps: ["schema lookup", "node scan"],
      indexes: []
    },
    result: {
      ...snapshot,
      source: "service",
      nodes: [snapshot.nodes[1]],
      edges: []
    }
  });

  const state = store.getState();
  assert.equal(state.filter.text, "");
  assert.equal(state.visibleSnapshot?.source, "service");
  assert.deepEqual(state.visibleSnapshot?.nodes.map((node) => node.id), ["573"]);
  assert.equal(state.lastExecutedQuery?.queryKind, "explain");
  assert.equal(state.lastExecutedQuery?.evidenceProvenance, "service");
  assert.equal(state.queryEvidence?.planKind, "node.find");

  const queryEvent = state.events.find((event) => event.id.startsWith("query-"));
  assert.ok(queryEvent);
  assert.equal(queryEvent.securityContext, "service-backed typed query via local gateway");
});

test("records unsupported query command state without changing graph result", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);
  store.rejectQueryCommand("write commands are not supported");

  const state = store.getState();
  assert.equal(state.queryStatus, "unsupported");
  assert.equal(state.queryEvidence?.provenance, "unsupported");
  assert.equal(state.visibleSnapshot?.nodes.length, 2);
});

test("does not record query execution before a snapshot is loaded", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.recordQueryExecution();

  const state = store.getState();
  assert.equal(state.lastExecutedQuery, null);
  assert.deepEqual(state.events.map((event) => event.id), ["idle"]);
});

test("records explicit query execution as a bounded local audit event", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);
  store.applyGraphFilter({
    text: "connection profiles",
    model: "",
    propertyKey: "",
    propertyValue: ""
  });
  store.recordQueryExecution();

  const state = store.getState();
  assert.equal(state.lastExecutedQuery?.workspace, "flight-deck-demo");
  assert.equal(state.lastExecutedQuery?.source, "fixture");
  assert.equal(state.lastExecutedQuery?.visibleNodes, 1);
  assert.equal(state.lastExecutedQuery?.visibleEdges, 0);

  const queryEvent = state.events.find((event) => event.id.startsWith("query-"));
  assert.ok(queryEvent);
  assert.equal(queryEvent.kind, "read");
  assert.equal(queryEvent.status, "fixture");
  assert.equal(queryEvent.workspace, "flight-deck-demo");
  assert.equal(queryEvent.securityContext, "fixture");
  assert.match(queryEvent.operationSummary ?? "", /text contains/);
});

test("keeps schema graph selections visible with projection labels", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadVisualProjection(visualProjection);
  store.loadSnapshot({
    ...snapshot,
    schemaNodeModels: [
      {
        name: "RoadmapItem",
        idField: "roadmapItemId",
        fields: [
          { name: "title", valueType: "string", required: true },
          { name: "status", valueType: "string", required: false }
        ]
      },
      {
        name: "WorkSlice",
        idField: "workSliceId",
        fields: [
          { name: "title", valueType: "string", required: true },
          { name: "status", valueType: "string", required: false }
        ]
      }
    ],
    schemaEdgeModels: [
      {
        name: "HAS_WORK_SLICE",
        fromModel: "RoadmapItem",
        toModel: "WorkSlice",
        idField: "hasWorkSliceId",
        fields: [{ name: "reason", valueType: "string", required: false }]
      }
    ]
  });

  store.selectGraphItem({ kind: "node", id: "schema-node:RoadmapItem" });
  assert.equal(store.getState().selectedItem?.label, "roadmap item");
  assert.equal(store.getState().selectedItem?.props.idField, "roadmapItemId");

  store.selectGraphItem({ kind: "edge", id: "schema-edge:HAS_WORK_SLICE:0" });
  assert.equal(store.getState().selectedItem?.label, "has work slice");
  assert.equal(store.getState().selectedItem?.props.fromModel, "RoadmapItem");
});

test("records schema view execution as a local schema projection", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);
  store.setGraphView("schema");
  store.recordQueryExecution();

  const state = store.getState();
  assert.equal(state.lastExecutedQuery?.graphView, "schema");
  assert.equal(state.lastExecutedQuery?.visibleNodes, 2);
  assert.equal(state.lastExecutedQuery?.visibleEdges, 1);

  const queryEvent = state.events.find((event) => event.id.startsWith("query-"));
  assert.ok(queryEvent);
  assert.equal(queryEvent.label, "schema projection executed: 2 models / 1 schema edges");
  assert.equal(queryEvent.operationSummary, "schema projection summary");
  assert.equal(queryEvent.resultState, "2 visible models, 1 visible schema edges");
});

test("changing graph view clears stale executed query context", () => {
  const store = createFlightDeckGraphStore(new MemoryStorage());
  store.loadSnapshot(snapshot);
  store.recordQueryExecution();
  assert.ok(store.getState().lastExecutedQuery);

  store.setGraphView("schema");
  assert.equal(store.getState().lastExecutedQuery, null);
});

test("does not persist workbench UI state, events, query context, or secret-like fields", () => {
  const storage = new MemoryStorage();
  const store = createFlightDeckGraphStore(storage);

  store.setConnectionDetailsOpen(true);
  store.selectWorkspacePanel("audit");
  store.setGraphView("schema");
  store.setExplainVisible(true);
  store.setProfileVisible(true);
  store.updateSettings({
    serviceBaseUrl: "http://admin:secret@127.0.0.1:3001?token=abc",
    workspace: "project-memory",
    useFixtureData: false
  });
  store.loadSnapshot(snapshot);
  store.recordQueryExecution();
  store.saveCurrentProfile();

  const persisted = storage.getItem(STORE_STORAGE_KEY);
  assert.ok(persisted);
  assert.equal(persisted.includes("connectionDetailsOpen"), false);
  assert.equal(persisted.includes("activeWorkspacePanel"), false);
  assert.equal(persisted.includes("graphView"), false);
  assert.equal(persisted.includes("queryCommand"), false);
  assert.equal(persisted.includes("queryEvidence"), false);
  assert.equal(persisted.includes("queryMessage"), false);
  assert.equal(persisted.includes("lastExecutedQuery"), false);
  assert.equal(persisted.includes("events"), false);
  assert.equal(persisted.includes("secret"), false);
  assert.equal(persisted.includes("token"), false);
  assert.equal(persisted.includes("admin:"), false);
});
