import { useSyncExternalStore } from "react";

import { filterSnapshot } from "./api";
import type {
  ConnectionProfile,
  ConnectionSettings,
  FlightDeckEvent,
  FlightDeckQueryResponse,
  FlightDeckSnapshot,
  FlightDeckVisualProjection,
  GraphView,
  GraphFilter,
  GraphSelection,
  NormalizedGraphSnapshot,
  QueryEvidence,
  QueryExecutionContext,
  SelectedGraphItem,
  WorkspacePanel
} from "./types";

export const STORE_STORAGE_KEY = "grm-flight-deck.graph-store.v1";
export const LEGACY_CONNECTION_STORAGE_KEY = "grm-flight-deck.connection.v2";

export const DEFAULT_CONNECTION_SETTINGS: ConnectionSettings = {
  serviceBaseUrl: "",
  mode: "local-anonymous-dev",
  workspace: "flight-deck-demo",
  limit: 5000,
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
  visualProjection: FlightDeckVisualProjection | null;
  selection: GraphSelection | null;
  selectedItem: SelectedGraphItem | null;
  connectionStatus: ConnectionStatusKind;
  status: string;
  statusDetail: string;
  lastError: string;
  events: FlightDeckEvent[];
  activeWorkspacePanel: WorkspacePanel;
  graphView: GraphView;
  connectionDetailsOpen: boolean;
  explainVisible: boolean;
  profileVisible: boolean;
  queryCommand: string;
  queryStatus: "idle" | "running" | "succeeded" | "failed" | "unsupported";
  queryMessage: string;
  queryEvidence: QueryEvidence | null;
  schemaFilter: string;
  lastExecutedQuery: QueryExecutionContext | null;
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
  loadVisualProjection: (projection: FlightDeckVisualProjection | null) => void;
  markConnectionFailed: (message: string) => void;
  selectGraphItem: (selection: GraphSelection | null) => void;
  selectWorkspacePanel: (panel: WorkspacePanel) => void;
  setGraphView: (view: GraphView) => void;
  setConnectionDetailsOpen: (open: boolean) => void;
  setExplainVisible: (visible: boolean) => void;
  setProfileVisible: (visible: boolean) => void;
  setQueryCommand: (command: string) => void;
  setSchemaFilter: (filter: string) => void;
  beginQueryCommand: () => void;
  rejectQueryCommand: (message: string) => void;
  failQueryCommand: (message: string) => void;
  applyQueryResponse: (response: FlightDeckQueryResponse) => void;
  recordQueryExecution: () => void;
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
    visualProjection: null,
    selection: null,
    selectedItem: null,
    connectionStatus: "idle",
    status: "Ready for a local service connection.",
    statusDetail: "",
    lastError: "",
    events: [idleEvent()],
    activeWorkspacePanel: "query",
    graphView: "data",
    connectionDetailsOpen: false,
    explainVisible: false,
    profileVisible: false,
    queryCommand: "node.find WorkSlice status=active limit=25",
    queryStatus: "idle",
    queryMessage: "Data commands execute through typed service requests.",
    queryEvidence: null,
    schemaFilter: "",
    lastExecutedQuery: null
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
      setState({ ...state, filter, lastExecutedQuery: null });
    },
    clearGraphFilter: () => {
      setState({ ...state, filter: DEFAULT_GRAPH_FILTER, lastExecutedQuery: null });
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
        lastExecutedQuery: null,
        events: eventsForSnapshot(snapshot, state.visibleSnapshot)
      });
    },
    loadVisualProjection: (projection) => {
      setState({
        ...state,
        visualProjection: projection
      });
    },
    markConnectionFailed: (message) => {
      setState({
        ...state,
        snapshot: null,
        normalizedSnapshot: null,
        visibleSnapshot: null,
        visualProjection: null,
        selection: null,
        selectedItem: null,
        connectionStatus: "failed",
        lastError: message,
        status: "Connection failed.",
        statusDetail: "",
        lastExecutedQuery: null,
        events: [idleEvent()]
      });
    },
    selectGraphItem: (selection) => {
      setState({ ...state, selection });
    },
    selectWorkspacePanel: (panel) => {
      setState({ ...state, activeWorkspacePanel: panel, selection: null });
    },
    setGraphView: (view) => {
      setState({ ...state, graphView: view, selection: null, lastExecutedQuery: null });
    },
    setConnectionDetailsOpen: (open) => {
      setState({ ...state, connectionDetailsOpen: open });
    },
    setExplainVisible: (visible) => {
      setState({ ...state, explainVisible: visible });
    },
    setProfileVisible: (visible) => {
      setState({ ...state, profileVisible: visible });
    },
    setQueryCommand: (command) => {
      setState({ ...state, queryCommand: command, queryStatus: "idle", queryMessage: "" });
    },
    setSchemaFilter: (filter) => {
      setState({ ...state, schemaFilter: filter, lastExecutedQuery: null });
    },
    beginQueryCommand: () => {
      setState({
        ...state,
        queryStatus: "running",
        queryMessage: "Executing typed service query...",
        queryEvidence: null
      });
    },
    rejectQueryCommand: (message) => {
      setState({
        ...state,
        queryStatus: "unsupported",
        queryMessage: message,
        queryEvidence: {
          provenance: "unsupported",
          label: "unsupported",
          unsupportedReason: message
        },
        lastExecutedQuery: null
      });
    },
    failQueryCommand: (message) => {
      setState({
        ...state,
        queryStatus: "failed",
        queryMessage: message,
        queryEvidence: null,
        lastExecutedQuery: null
      });
    },
    applyQueryResponse: (response) => {
      const result = response.result;
      const snapshot = {
        ...result,
        partialReason: response.partialReason ?? result.partialReason
      };
      const context = queryExecutionContext(
        snapshot,
        snapshot,
        DEFAULT_GRAPH_FILTER,
        "data",
        response.command,
        response.kind,
        response.queryShape,
        response.evidence.provenance
      );
      const event: FlightDeckEvent = {
        id: context.id,
        kind: "read",
        label: `${response.queryShape} ${response.kind}: ${snapshot.nodes.length} nodes / ${snapshot.edges.length} edges`,
        operationSummary: serviceCommandSummary(response.command),
        securityContext:
          response.evidence.provenance === "service"
            ? "service-backed typed query via local gateway"
            : response.evidence.label,
        resultState: response.partialReason ?? result.partialReason ?? executionResultState(context),
        status: "observed",
        workspace: response.workspace
      };

      setState({
        ...state,
        snapshot,
        normalizedSnapshot: normalizeSnapshot(snapshot),
        filter: DEFAULT_GRAPH_FILTER,
        selection: null,
        lastExecutedQuery: context,
        queryStatus: "succeeded",
        queryMessage: response.evidence.label,
        queryEvidence: response.evidence,
        events: [...state.events.filter((item) => item.id !== "idle"), event]
      });
    },
    recordQueryExecution: () => {
      if (!state.snapshot) {
        return;
      }
      const context = queryExecutionContext(
        state.snapshot,
        state.visibleSnapshot,
        state.filter,
        state.graphView,
        state.queryCommand,
        "local_summary",
        state.graphView === "schema" ? "schema projection" : "bounded snapshot filter",
        "local_summary"
      );
      const status = context.source === "fixture" ? "fixture" : "observed";
      setState({
        ...state,
        lastExecutedQuery: context,
        events: [
          ...state.events.filter((item) => item.id !== "idle"),
          {
            id: context.id,
            kind: "read",
            label: executionEventLabel(context),
            operationSummary: predicateEventSummary(context.filter, context.graphView),
            securityContext:
              context.source === "fixture"
                ? "fixture"
                : context.source === "service"
                  ? "service snapshot via local gateway"
                  : "local view without loaded snapshot",
            resultState: context.partialReason
              ? `partial: ${context.partialReason}`
              : executionResultState(context),
            status,
            workspace: context.workspace
          }
        ]
      });
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
        visualProjection: null,
        selection: null,
        selectedItem: null,
        lastExecutedQuery: null,
        queryStatus: "idle",
        queryMessage: "",
        queryEvidence: null,
        events: [idleEvent()]
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
  const selectedItem = selection ? selectedItemFromSnapshot(selection, visibleSnapshot, state.visualProjection) : null;
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
  snapshot: FlightDeckSnapshot | null,
  projection: FlightDeckVisualProjection | null
): SelectedGraphItem | null {
  if (!snapshot) {
    return null;
  }
  if (selection.kind === "node") {
    const schemaModel = selection.id.startsWith("schema-node:")
      ? selection.id.slice("schema-node:".length)
      : null;
    if (schemaModel) {
      const model = schemaNodeModels(snapshot).find((item) => item.name === schemaModel);
      const hint = projection?.nodeModels.find((item) => item.model === schemaModel);
      return model
        ? {
            kind: "node",
            id: selection.id,
            label: hint?.label ?? model.name,
            model: model.name,
            props: {
              kind: "node_model",
              idField: model.idField,
              fields: model.fields.map((field) => `${field.name}${field.required ? "" : "?"}: ${field.valueType}`)
            }
          }
        : null;
    }

    const node = snapshot.nodes.find((item) => item.id === selection.id);
    return node ? { kind: "node", id: node.id, label: node.label, model: node.model, props: node.props } : null;
  }

  const schemaEdge = selection.id.startsWith("schema-edge:")
    ? schemaEdgeFromSelection(snapshot, selection.id)
    : null;
  if (schemaEdge) {
    const hint = projection?.edgeModels.find((item) => item.model === schemaEdge.model);
    const model = schemaEdgeModels(snapshot).find(
      (item) => item.name === schemaEdge.model && item.fromModel === schemaEdge.fromModel && item.toModel === schemaEdge.toModel
    );
    return {
      kind: "edge",
      id: selection.id,
      label: hint?.label ?? schemaEdge.model,
      model: schemaEdge.model,
      props: {
        kind: "edge_model",
        fromModel: schemaEdge.fromModel,
        toModel: schemaEdge.toModel,
        fields: model?.fields.map((field) => `${field.name}${field.required ? "" : "?"}: ${field.valueType}`) ?? []
      }
    };
  }

  const edge = snapshot.edges.find((item) => item.id === selection.id);
  return edge ? { kind: "edge", id: edge.id, label: edge.model, model: edge.model, props: edge.props } : null;
}

function schemaNodeModels(snapshot: FlightDeckSnapshot) {
  return snapshot.schemaNodeModels ?? snapshot.nodeModels.map((name) => ({ name, idField: "id", fields: [] }));
}

function schemaEdgeModels(snapshot: FlightDeckSnapshot) {
  return snapshot.schemaEdgeModels ?? snapshot.schemaEdges?.map((edge) => ({
    name: edge.model,
    fromModel: edge.fromModel,
    toModel: edge.toModel,
    idField: "id",
    fields: []
  })) ?? [];
}

function schemaEdgeFromSelection(snapshot: FlightDeckSnapshot, id: string) {
  const [, model, index] = id.split(":");
  const numericIndex = Number(index);
  const schemaEdges = snapshot.schemaEdges ?? schemaEdgeModels(snapshot).map((edge) => ({
    model: edge.name,
    fromModel: edge.fromModel,
    toModel: edge.toModel
  }));
  if (Number.isInteger(numericIndex) && schemaEdges[numericIndex]?.model === model) {
    return schemaEdges[numericIndex];
  }
  return schemaEdges.find((edge) => edge.model === model) ?? null;
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
      ? selection.id.startsWith("schema-node:")
        ? schemaNodeModels(snapshot).some((node) => `schema-node:${node.name}` === selection.id)
        : snapshot.nodes.some((node) => node.id === selection.id)
      : selection.id.startsWith("schema-edge:")
        ? schemaEdgeFromSelection(snapshot, selection.id) !== null
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
      operationSummary: "bounded snapshot read",
      securityContext: snapshot.source === "fixture" ? "fixture" : "service snapshot via local gateway",
      resultState: snapshot.partialReason ?? "snapshot loaded",
      workspace: snapshot.workspace,
      status: snapshot.source === "fixture" ? "fixture" : "observed"
    },
    {
      id: "filter-boundary",
      kind: "edge-traversed",
      label: `${visibleSnapshot?.edges.length ?? snapshot.edges.length} edges visible`,
      operationSummary: "local view filter",
      securityContext: snapshot.source === "fixture" ? "fixture" : "client-side projection",
      resultState: `${visibleSnapshot?.nodes.length ?? snapshot.nodes.length} visible nodes`,
      workspace: snapshot.workspace,
      status: snapshot.source === "fixture" ? "fixture" : "observed"
    }
  ];
}

function idleEvent(): FlightDeckEvent {
  return {
    id: "idle",
    kind: "read",
    label: "event hook idle",
    operationSummary: "no query executed",
    securityContext: "local UI event buffer",
    resultState: "waiting",
    status: "fixture"
  };
}

function queryExecutionContext(
  snapshot: FlightDeckSnapshot | null,
  visibleSnapshot: FlightDeckSnapshot | null,
  filter: GraphFilter,
  graphView: GraphView,
  commandText = "",
  queryKind: QueryExecutionContext["queryKind"] = "local_summary",
  queryShape = graphView === "schema" ? "schema projection" : "bounded snapshot filter",
  evidenceProvenance: QueryExecutionContext["evidenceProvenance"] = "local_summary"
): QueryExecutionContext {
  const schemaEdgeCount = schemaEdgeCountForSnapshot(snapshot);
  const sourceNodes = graphView === "schema" ? snapshot?.nodeModels.length ?? 0 : snapshot?.nodes.length ?? 0;
  const sourceEdges = graphView === "schema" ? schemaEdgeCount : snapshot?.edges.length ?? 0;
  const visibleNodes = graphView === "schema" ? snapshot?.nodeModels.length ?? 0 : visibleSnapshot?.nodes.length ?? 0;
  const visibleEdges = graphView === "schema" ? schemaEdgeCount : visibleSnapshot?.edges.length ?? 0;

  return {
    id: `query-${Date.now()}`,
    workspace: snapshot?.workspace ?? "not loaded",
    source: snapshot?.source ?? "none",
    graphView,
    commandText,
    queryKind,
    queryShape,
    evidenceProvenance,
    filter,
    sourceNodes,
    sourceEdges,
    visibleNodes,
    visibleEdges,
    limit: snapshot?.modelLimit ?? null,
    omittedEdges: visibleSnapshot?.omittedEdges ?? snapshot?.omittedEdges ?? 0,
    partialReason: visibleSnapshot?.partialReason ?? snapshot?.partialReason
  };
}

function serviceCommandSummary(command: string): string {
  const trimmed = command.trim().replace(/\s+/g, " ");
  if (trimmed.length <= 120) {
    return trimmed;
  }
  return `${trimmed.slice(0, 117)}...`;
}

function predicateEventSummary(filter: GraphFilter, graphView: GraphView): string {
  if (graphView === "schema") {
    return "schema projection summary";
  }

  const parts = [
    filter.model.trim() ? `model=${filter.model.trim()}` : "",
    filter.text.trim() ? `text contains "${filter.text.trim()}"` : "",
    filter.propertyKey.trim()
      ? `${filter.propertyKey.trim()} contains ${filter.propertyValue.trim() || "*"}`
      : ""
  ].filter(Boolean);

  return parts.length > 0 ? parts.join(", ") : "bounded snapshot, no filter";
}

function executionEventLabel(context: QueryExecutionContext): string {
  if (context.graphView === "schema") {
    return `schema projection executed: ${context.visibleNodes} models / ${context.visibleEdges} schema edges`;
  }

  return `query executed: ${context.visibleNodes} nodes / ${context.visibleEdges} edges`;
}

function executionResultState(context: QueryExecutionContext): string {
  if (context.graphView === "schema") {
    return `${context.visibleNodes} visible models, ${context.visibleEdges} visible schema edges`;
  }

  return `${context.visibleNodes} visible nodes, ${context.omittedEdges} omitted edges`;
}

function schemaEdgeCountForSnapshot(snapshot: FlightDeckSnapshot | null): number {
  if (!snapshot) {
    return 0;
  }
  if (snapshot.schemaEdges) {
    return snapshot.schemaEdges.length;
  }

  const nodesById = new Map(snapshot.nodes.map((node) => [node.id, node]));
  const inferredEdges = new Set<string>();
  for (const edge of snapshot.edges) {
    const from = nodesById.get(edge.from);
    const to = nodesById.get(edge.to);
    if (from && to) {
      inferredEdges.add(`${edge.model}:${from.model}:${to.model}`);
    }
  }
  return inferredEdges.size;
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
  return Math.min(25000, Math.max(1, Math.trunc(limit)));
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
    if (looksLikeSecretLocalPath(url.pathname)) {
      url.pathname = "/";
    }
    return url.toString().replace(/\/$/, "");
  } catch {
    const withoutCredentials = raw
      .replace(/[?#].*$/, "")
      .replace(/^([a-z][a-z0-9+.-]*:\/\/)[^/?#@]+@/i, "$1")
      .replace(/^(\/\/)[^/?#@]+@/, "$1")
      .replace(/\/$/, "");
    return withoutCredentials.replace(/\/[^/]*(?:key|cert|certificate|credential|secret|fingerprint|policy)[^/]*$/i, "");
  }
}

function looksLikeSecretLocalPath(pathname: string): boolean {
  return /(?:key|cert|certificate|credential|secret|fingerprint|policy)/i.test(pathname);
}
