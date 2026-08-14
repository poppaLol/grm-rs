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
