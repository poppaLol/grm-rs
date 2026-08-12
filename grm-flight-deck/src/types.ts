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
}

export type SelectedGraphItem =
  | { kind: "node"; label: string; model: string; props: Record<string, JsonValue> }
  | { kind: "edge"; label: string; model: string; props: Record<string, JsonValue> };
