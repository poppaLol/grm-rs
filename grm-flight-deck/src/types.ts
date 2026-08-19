export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type ConnectionMode =
  | "fixture"
  | "anonymous_local"
  | "docker_local_insecure"
  | "secured"
  | "local-anonymous-dev";

export interface ConnectionSettings {
  serviceBaseUrl: string;
  mode: ConnectionMode;
  workspace: string;
  limit: number;
  useFixtureData: boolean;
}

export interface ConnectionProfile {
  id: string;
  name: string;
  settings: ConnectionSettings;
}

export interface FlightDeckNode {
  id: string;
  model: string;
  label: string;
  props: Record<string, JsonValue>;
}

export interface FlightDeckEdge {
  id: string;
  model: string;
  from: string;
  to: string;
  props: Record<string, JsonValue>;
}

export interface FlightDeckSnapshot {
  workspace: string;
  nodeModels: string[];
  edgeModels: string[];
  schemaEdges?: FlightDeckSchemaEdge[];
  nodes: FlightDeckNode[];
  edges: FlightDeckEdge[];
  modelLimit: number;
  omittedEdges: number;
  source: "service" | "fixture";
  partialReason?: string;
}

export interface FlightDeckSecurityStatus {
  securityProfile: "anonymous_local" | "docker_local_insecure" | "secured" | "fixture" | "unknown";
  identityStatus:
    | "anonymous_local"
    | "docker_local_insecure"
    | "authenticated_principal"
    | "fixture"
    | "unknown";
  principal?: FlightDeckPrincipal | null;
  authenticationMethod?: string | null;
  policyVersion?: string | null;
}

export interface FlightDeckPrincipal {
  issuer: string;
  subject: string;
}

export interface FlightDeckSchemaEdge {
  model: string;
  fromModel: string;
  toModel: string;
}

export interface GraphFilter {
  text: string;
  model: string;
  propertyKey: string;
  propertyValue: string;
}

export interface FlightDeckEvent {
  id: string;
  kind: "read" | "write" | "node-created" | "edge-traversed";
  label: string;
  status: "fixture" | "pending" | "observed";
  operationSummary?: string;
  securityContext?: string;
  resultState?: string;
  workspace?: string;
}

export type SelectedGraphItem =
  | { kind: "node"; id: string; label: string; model: string; props: Record<string, JsonValue> }
  | { kind: "edge"; id: string; label: string; model: string; props: Record<string, JsonValue> };

export type GraphSelection =
  | { kind: "node"; id: string }
  | { kind: "edge"; id: string };

export interface NormalizedGraphSnapshot {
  workspace: string;
  nodeModels: string[];
  edgeModels: string[];
  schemaEdges?: FlightDeckSchemaEdge[];
  nodesById: Record<string, FlightDeckNode>;
  edgesById: Record<string, FlightDeckEdge>;
  nodeIds: string[];
  edgeIds: string[];
  modelLimit: number;
  omittedEdges: number;
  source: "service" | "fixture";
  partialReason?: string;
}

export type WorkspacePanel = "query" | "audit";
export type GraphView = "data" | "schema";

export interface QueryExecutionContext {
  id: string;
  workspace: string;
  source: "service" | "fixture" | "none";
  graphView: GraphView;
  filter: GraphFilter;
  sourceNodes: number;
  sourceEdges: number;
  visibleNodes: number;
  visibleEdges: number;
  limit: number | null;
  omittedEdges: number;
  partialReason?: string;
}
