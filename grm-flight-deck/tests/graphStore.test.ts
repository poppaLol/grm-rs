import assert from "node:assert/strict";
import test from "node:test";

import {
  createFlightDeckGraphStore,
  LEGACY_CONNECTION_STORAGE_KEY,
  normalizeSnapshot,
  STORE_STORAGE_KEY
} from "../src/graphStore";
import type { FlightDeckSnapshot } from "../src/types";

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
