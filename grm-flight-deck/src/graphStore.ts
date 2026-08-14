import { useSyncExternalStore } from "react";

import { filterSnapshot } from "./api";
import type {
  ConnectionProfile,
  ConnectionSettings,
  FlightDeckEvent,
  FlightDeckSnapshot,
  GraphFilter,
  GraphSelection,
  NormalizedGraphSnapshot,
  SelectedGraphItem
} from "./types";

export const STORE_STORAGE_KEY = "grm-flight-deck.graph-store.v1";
export const LEGACY_CONNECTION_STORAGE_KEY = "grm-flight-deck.connection.v2";

export const DEFAULT_CONNECTION_SETTINGS: ConnectionSettings = {
  serviceBaseUrl: "",
  mode: "local-anonymous-dev",
  workspace: "flight-deck-demo",
  limit: 50,
  useFixtureData: true
};

export const DEFAULT_GRAPH_FILTER: GraphFilter = {
  text: "",
  model: "",
  propertyKey: "",
  propertyValue: ""
};

export type ConnectionStatusKind = "idle" | "loading" | "connected" | "failed";

interface PersistedStore {
  version: 1;
  selectedProfileId: string;
  profiles: ConnectionProfile[];
}

export interface FlightDeckGraphStoreState {
  profiles: ConnectionProfile[];
  selectedProfileId: string;
  draftProfileName: string;
  settings: ConnectionSettings;
  filter: GraphFilter;
  snapshot: FlightDeckSnapshot | null;
  normalizedSnapshot: NormalizedGraphSnapshot | null;
  visibleSnapshot: FlightDeckSnapshot | null;
  selection: GraphSelection | null;
  selectedItem: SelectedGraphItem | null;
  connectionStatus: ConnectionStatusKind;
  status: string;
  statusDetail: string;
  lastError: string;
  events: FlightDeckEvent[];
}

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

type Listener = () => void;

export interface FlightDeckGraphStore {
  getState: () => FlightDeckGraphStoreState;
  subscribe: (listener: Listener) => () => void;
  updateSettings: (patch: Partial<ConnectionSettings>) => void;
  updateDraftProfileName: (name: string) => void;
  saveCurrentProfile: () => void;
  createProfile: () => void;
  selectProfile: (profileId: string) => void;
  applyGraphFilter: (filter: GraphFilter) => void;
  clearGraphFilter: () => void;
  beginSnapshotLoad: (useFixtureData: boolean) => void;
  loadSnapshot: (snapshot: FlightDeckSnapshot) => void;
  markConnectionFailed: (message: string) => void;
  selectGraphItem: (selection: GraphSelection | null) => void;
  applyExecutionEvent: (event: FlightDeckEvent) => void;
  clearWorkspace: () => void;
}

export function createFlightDeckGraphStore(storage?: StorageLike): FlightDeckGraphStore {
  const restored = restoreInitialProfiles(storage);
  const initialProfile = restored.selectedProfile;
  let state: FlightDeckGraphStoreState = deriveState({
    profiles: restored.profiles,
    selectedProfileId: initialProfile.id,
    draftProfileName: initialProfile.name,
    settings: initialProfile.settings,
    filter: DEFAULT_GRAPH_FILTER,
    snapshot: null,
    normalizedSnapshot: null,
    visibleSnapshot: null,
    selection: null,
    selectedItem: null,
    connectionStatus: "idle",
    status: "Ready for a local service connection.",
    statusDetail: "",
    lastError: "",
    events: [{ id: "idle", kind: "read", label: "event hook idle", status: "fixture" }]
  });
  const listeners = new Set<Listener>();

  const emit = () => {
    for (const listener of listeners) {
      listener();
    }
  };

  const setState = (next: FlightDeckGraphStoreState) => {
    state = deriveState(next);
    persistProfiles(storage, state);
    emit();
  };

  return {
    getState: () => state,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    updateSettings: (patch) => {
      const settings = sanitizeConnectionSettings({ ...state.settings, ...patch });
      setState({
        ...state,
        settings
      });
    },
    updateDraftProfileName: (name) => {
      setState({ ...state, draftProfileName: name });
    },
    saveCurrentProfile: () => {
      const profile = sanitizeConnectionProfile({
        id: state.selectedProfileId,
        name: state.draftProfileName.trim() || "Local workspace",
        settings: state.settings
      });

      setState({
        ...state,
        profiles: state.profiles.map((item) => (item.id === profile.id ? profile : item)),
        selectedProfileId: profile.id,
        draftProfileName: profile.name,
        settings: profile.settings
      });
    },
    createProfile: () => {
      const name = state.draftProfileName.trim() || "Local workspace";
      const profile = sanitizeConnectionProfile({
        id: uniqueProfileId(name, state.profiles.map((item) => item.id)),
        name,
        settings: state.settings
      });

      setState({
        ...state,
        profiles: [...state.profiles, profile],
        selectedProfileId: profile.id,
        draftProfileName: profile.name,
        settings: profile.settings,
        selection: null
      });
    },
    selectProfile: (profileId) => {
      const profile = state.profiles.find((item) => item.id === profileId);
      if (!profile) {
        return;
      }
      setState({
        ...state,
        selectedProfileId: profile.id,
        draftProfileName: profile.name,
        settings: profile.settings,
        selection: null
      });
    },
    applyGraphFilter: (filter) => {
      setState({ ...state, filter });
    },
    clearGraphFilter: () => {
      setState({ ...state, filter: DEFAULT_GRAPH_FILTER });
    },
    beginSnapshotLoad: (useFixtureData) => {
      setState({
        ...state,
        connectionStatus: "loading",
        selection: null,
        lastError: "",
        status: useFixtureData ? "Loading fixture snapshot..." : "Connecting to service...",
        statusDetail: ""
      });
    },
    loadSnapshot: (snapshot) => {
      setState({
        ...state,
        snapshot,
        normalizedSnapshot: normalizeSnapshot(snapshot),
        connectionStatus: "connected",
        status: `${snapshot.source} snapshot:`,
        statusDetail: `${snapshot.nodes.length} nodes / ${snapshot.edges.length} edges / limit ${snapshot.modelLimit}`,
        lastError: "",
        events: eventsForSnapshot(snapshot, state.visibleSnapshot)
      });
    },
    markConnectionFailed: (message) => {
      setState({
        ...state,
        snapshot: null,
        normalizedSnapshot: null,
        visibleSnapshot: null,
        selection: null,
        selectedItem: null,
        connectionStatus: "failed",
        lastError: message,
        status: "Connection failed.",
        statusDetail: "",
        events: [{ id: "idle", kind: "read", label: "event hook idle", status: "fixture" }]
      });
    },
    selectGraphItem: (selection) => {
      setState({ ...state, selection });
    },
    applyExecutionEvent: (event) => {
      setState({ ...state, events: [...state.events.filter((item) => item.id !== "idle"), event] });
    },
    clearWorkspace: () => {
      setState({
        ...state,
        snapshot: null,
        normalizedSnapshot: null,
        visibleSnapshot: null,
        selection: null,
        selectedItem: null,
        events: [{ id: "idle", kind: "read", label: "event hook idle", status: "fixture" }]
      });
    }
  };
}

export function useFlightDeckGraphStore(store: FlightDeckGraphStore): FlightDeckGraphStoreState {
  return useSyncExternalStore(store.subscribe, store.getState, store.getState);
}

export function normalizeSnapshot(snapshot: FlightDeckSnapshot): NormalizedGraphSnapshot {
  const nodesById = Object.fromEntries(snapshot.nodes.map((node) => [node.id, node]));
  const edgesById = Object.fromEntries(snapshot.edges.map((edge) => [edge.id, edge]));

  return {
    workspace: snapshot.workspace,
    nodeModels: snapshot.nodeModels,
    edgeModels: snapshot.edgeModels,
    schemaEdges: snapshot.schemaEdges,
    nodesById,
    edgesById,
    nodeIds: snapshot.nodes.map((node) => node.id),
    edgeIds: snapshot.edges.map((edge) => edge.id),
    modelLimit: snapshot.modelLimit,
    omittedEdges: snapshot.omittedEdges,
    source: snapshot.source,
    partialReason: snapshot.partialReason
  };
}

export function sanitizeConnectionProfile(profile: ConnectionProfile): ConnectionProfile {
  return {
    id: stableProfileId(profile.id || profile.name),
    name: profile.name.trim() || "Local workspace",
    settings: sanitizeConnectionSettings(profile.settings)
  };
}

function deriveState(state: FlightDeckGraphStoreState): FlightDeckGraphStoreState {
  const visibleSnapshot = state.snapshot ? filterSnapshot(state.snapshot, state.filter) : null;
  const selection = keepSelectionIfVisible(state.selection, visibleSnapshot);
  const selectedItem = selection ? selectedItemFromSnapshot(selection, visibleSnapshot) : null;
  const events = state.snapshot
    ? [
        ...eventsForSnapshot(state.snapshot, visibleSnapshot),
        ...state.events.filter(
          (event) => event.id !== "idle" && event.id !== "snapshot-read" && event.id !== "filter-boundary"
        )
      ]
    : state.events;

  return {
    ...state,
    visibleSnapshot,
    normalizedSnapshot: state.snapshot ? normalizeSnapshot(state.snapshot) : null,
    selection,
    selectedItem,
    events
  };
}

function selectedItemFromSnapshot(
  selection: GraphSelection,
  snapshot: FlightDeckSnapshot | null
): SelectedGraphItem | null {
  if (!snapshot) {
    return null;
  }
  if (selection.kind === "node") {
    const node = snapshot.nodes.find((item) => item.id === selection.id);
    return node ? { kind: "node", id: node.id, label: node.label, model: node.model, props: node.props } : null;
  }

  const edge = snapshot.edges.find((item) => item.id === selection.id);
  return edge ? { kind: "edge", id: edge.id, label: edge.model, model: edge.model, props: edge.props } : null;
}

function keepSelectionIfVisible(
  selection: GraphSelection | null,
  snapshot: FlightDeckSnapshot | null
): GraphSelection | null {
  if (!selection || !snapshot) {
    return null;
  }
  const visible =
    selection.kind === "node"
      ? snapshot.nodes.some((node) => node.id === selection.id)
      : snapshot.edges.some((edge) => edge.id === selection.id);
  return visible ? selection : null;
}

function eventsForSnapshot(
  snapshot: FlightDeckSnapshot,
  visibleSnapshot: FlightDeckSnapshot | null
): FlightDeckEvent[] {
  return [
    {
      id: "snapshot-read",
      kind: "read",
      label: `${snapshot.nodes.length} nodes observed`,
      status: snapshot.source === "fixture" ? "fixture" : "observed"
    },
    {
      id: "filter-boundary",
      kind: "edge-traversed",
      label: `${visibleSnapshot?.edges.length ?? snapshot.edges.length} edges visible`,
      status: snapshot.source === "fixture" ? "fixture" : "observed"
    }
  ];
}

function restoreInitialProfiles(storage?: StorageLike): {
  profiles: ConnectionProfile[];
  selectedProfile: ConnectionProfile;
} {
  const restored = readPersistedStore(storage);
  if (restored) {
    const selectedProfile =
      restored.profiles.find((profile) => profile.id === restored.selectedProfileId) ??
      restored.profiles[0];
    return { profiles: restored.profiles, selectedProfile };
  }

  const selectedProfile = {
    id: "local-workspace",
    name: "Local workspace",
    settings: readLegacySettings(storage)
  };
  return { profiles: [selectedProfile], selectedProfile };
}

function readPersistedStore(storage?: StorageLike): PersistedStore | null {
  const raw = storage?.getItem(STORE_STORAGE_KEY);
  if (!raw) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as Partial<PersistedStore>;
    const profiles = Array.isArray(parsed.profiles)
      ? parsed.profiles.map(sanitizeConnectionProfile)
      : [];
    if (!parsed.selectedProfileId || profiles.length === 0) {
      return null;
    }
    return {
      version: 1,
      selectedProfileId: stableProfileId(parsed.selectedProfileId),
      profiles
    };
  } catch {
    return null;
  }
}

function readLegacySettings(storage?: StorageLike): ConnectionSettings {
  const raw = storage?.getItem(LEGACY_CONNECTION_STORAGE_KEY);
  if (!raw) {
    return DEFAULT_CONNECTION_SETTINGS;
  }

  try {
    return sanitizeConnectionSettings({
      ...DEFAULT_CONNECTION_SETTINGS,
      ...JSON.parse(raw)
    });
  } catch {
    return DEFAULT_CONNECTION_SETTINGS;
  }
}

function persistProfiles(storage: StorageLike | undefined, state: FlightDeckGraphStoreState) {
  if (!storage) {
    return;
  }
  const payload: PersistedStore = {
    version: 1,
    selectedProfileId: state.selectedProfileId,
    profiles: state.profiles.map(sanitizeConnectionProfile)
  };
  storage.setItem(STORE_STORAGE_KEY, JSON.stringify(payload));
}

function sanitizeConnectionSettings(settings: ConnectionSettings): ConnectionSettings {
  return {
    serviceBaseUrl: sanitizeServiceBaseUrl(settings.serviceBaseUrl),
    mode: settings.mode ?? DEFAULT_CONNECTION_SETTINGS.mode,
    workspace: String(settings.workspace ?? DEFAULT_CONNECTION_SETTINGS.workspace),
    limit: boundedLimit(settings.limit),
    useFixtureData: Boolean(settings.useFixtureData)
  };
}

function boundedLimit(limit: number): number {
  if (!Number.isFinite(limit)) {
    return DEFAULT_CONNECTION_SETTINGS.limit;
  }
  return Math.min(1000, Math.max(1, Math.trunc(limit)));
}

function stableProfileId(value: string): string {
  const trimmed = value.trim().toLowerCase();
  const slug = trimmed.replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return slug || "local-workspace";
}

function uniqueProfileId(value: string, existingIds: string[]): string {
  const base = stableProfileId(value);
  const existing = new Set(existingIds);
  if (!existing.has(base)) {
    return base;
  }

  for (let index = 2; ; index += 1) {
    const candidate = `${base}-${index}`;
    if (!existing.has(candidate)) {
      return candidate;
    }
  }
}

function sanitizeServiceBaseUrl(value: unknown): string {
  const raw = String(value ?? "").trim();
  if (!raw) {
    return "";
  }

  try {
    const url = new URL(raw);
    url.username = "";
    url.password = "";
    url.search = "";
    url.hash = "";
    return url.toString().replace(/\/$/, "");
  } catch {
    return raw
      .replace(/[?#].*$/, "")
      .replace(/^([a-z][a-z0-9+.-]*:\/\/)[^/?#@]+@/i, "$1")
      .replace(/^(\/\/)[^/?#@]+@/, "$1")
      .replace(/\/$/, "");
  }
}
