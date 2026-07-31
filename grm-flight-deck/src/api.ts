import { fixtureSnapshot } from "./fixtures";
import type { ConnectionSettings, FlightDeckSnapshot, GraphFilter, JsonValue } from "./types";

export async function fetchSnapshot(
  settings: ConnectionSettings,
  signal?: AbortSignal
): Promise<FlightDeckSnapshot> {
  if (settings.useFixtureData) {
    return {
      ...fixtureSnapshot,
      workspace: settings.workspace.trim() || fixtureSnapshot.workspace,
      modelLimit: settings.limit
    };
  }

  const encodedWorkspace = encodeURIComponent(settings.workspace.trim());
  const baseUrl = settings.serviceBaseUrl.trim().replace(/\/$/, "");
  const response = await fetch(
    `${baseUrl}/api/workspaces/${encodedWorkspace}/snapshot?limit=${settings.limit}`,
    { signal }
  );

  if (!response.ok) {
    throw new Error(`snapshot request failed: HTTP ${response.status}`);
  }

  const snapshot = await response.json() as FlightDeckSnapshot;
  return { ...snapshot, source: "service" };
}

export function filterSnapshot(
  snapshot: FlightDeckSnapshot,
  filter: GraphFilter
): FlightDeckSnapshot {
  const text = filter.text.trim().toLowerCase();
  const model = filter.model.trim();
  const propertyKey = filter.propertyKey.trim();
  const propertyValue = filter.propertyValue.trim().toLowerCase();

  if (!text && !model && !propertyKey && !propertyValue) {
    return snapshot;
  }

  const nodeMatches = snapshot.nodes.filter((node) => {
    if (model && node.model !== model) {
      return false;
    }
    return matchesGraphItem(node.id, node.model, node.label, node.props, text, propertyKey, propertyValue);
  });
  const matchedNodeIds = new Set(nodeMatches.map((node) => node.id));

  const edgeMatches = snapshot.edges.filter((edge) => {
    const endpointsMatch = matchedNodeIds.has(edge.from) && matchedNodeIds.has(edge.to);
    const edgeMatchesFilter =
      (!model || edge.model === model) &&
      matchesGraphItem(edge.id, edge.model, edge.model, edge.props, text, propertyKey, propertyValue);

    return endpointsMatch || edgeMatchesFilter;
  });

  const edgeEndpointIds = new Set<string>();
  for (const edge of edgeMatches) {
    edgeEndpointIds.add(edge.from);
    edgeEndpointIds.add(edge.to);
  }

  const nodes = snapshot.nodes.filter(
    (node) => matchedNodeIds.has(node.id) || edgeEndpointIds.has(node.id)
  );

  return {
    ...snapshot,
    nodes,
    edges: edgeMatches,
    partialReason: snapshot.partialReason
  };
}

function matchesGraphItem(
  id: string,
  model: string,
  label: string,
  props: Record<string, JsonValue>,
  text: string,
  propertyKey: string,
  propertyValue: string
): boolean {
  if (text) {
    const haystack = `${id} ${model} ${label} ${JSON.stringify(props)}`.toLowerCase();
    if (!haystack.includes(text)) {
      return false;
    }
  }

  if (propertyKey) {
    const value = props[propertyKey];
    if (value === undefined) {
      return false;
    }
    if (propertyValue && !String(value).toLowerCase().includes(propertyValue)) {
      return false;
    }
  }

  return true;
}
