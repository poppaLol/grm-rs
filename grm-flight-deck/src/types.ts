export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type ConnectionMode = "local-anonymous-dev";

export interface ConnectionSettings {
  serviceBaseUrl: string;
  mode: ConnectionMode;
  workspace: string;
  limit: number;
  useFixtureData: boolean;
}

export interface FlightDeckNode {
  id: number;
  model: string;
  label: string;
  props: Record<string, JsonValue>;
}

export interface FlightDeckEdge {
  id: number;
  model: string;
  from: number;
  to: number;
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
