import type { FlightDeckVisualProjection } from "./types";

export const VISUAL_OVERLAY_STORAGE_KEY = "grm-flight-deck.visual-overlays.v1";

export type VisualNodeShape = "generated" | "dot" | "card" | "hex" | "diamond" | "lane";
export type VisualDetailDensity = "generated" | "compact" | "standard" | "rich";
export type VisualNodeRole =
  | "generated"
  | "anchor"
  | "context"
  | "evidence"
  | "decision"
  | "risk"
  | "actor"
  | "object"
  | "process";
export type VisualEdgeStyle = "generated" | "directed" | "dependency" | "evidence" | "warning";
export type VisualDirectionEmphasis = "generated" | "normal" | "strong" | "muted";
export type VisualLineWeight = "generated" | "fine" | "normal" | "strong";
export type VisualLineStyle = "generated" | "solid" | "dashed" | "dotted";
export type VisualLabelVisibility = "generated" | "hidden" | "self-loops" | "always";
export type VisualContainerRender = "generated" | "region" | "card" | "lane" | "section";
export type VisualContainerCollapse = "generated" | "expanded" | "collapsed";
export type VisualLayoutMode = "edge-network" | "container-map";
export type VisualLayoutStyle = "generated" | "force" | "groups" | "hierarchy" | "grid";

export interface VisualProjectionNodeOverlay {
  label?: string;
  colorToken?: string;
  group?: string;
  glyph?: string;
  shape?: string;
  detailDensity?: string;
  visualRole?: string;
}

export interface VisualProjectionEdgeOverlay {
  label?: string;
  styleToken?: string;
  directionEmphasis?: string;
  lineWeight?: string;
  lineStyle?: string;
  labelVisibility?: string;
}

export interface VisualProjectionContainerOverlay {
  parentModel?: string;
  childModel?: string;
  viaEdgeModel?: string;
  renderAs?: string;
  collapse?: string;
  styleToken?: string;
}

export interface VisualProjectionLayoutOverlay {
  mode?: VisualLayoutMode | string;
  style?: VisualLayoutStyle | string;
}

export interface VisualProjectionOverlay {
  version: 1;
  profileId: string;
  workspace: string;
  nodeModels: Record<string, VisualProjectionNodeOverlay>;
  edgeModels: Record<string, VisualProjectionEdgeOverlay>;
  containers: Record<string, VisualProjectionContainerOverlay>;
  layout: VisualProjectionLayoutOverlay;
}

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function emptyVisualProjectionOverlay(profileId: string, workspace: string): VisualProjectionOverlay {
  return {
    version: 1,
    profileId: scopedValue(profileId),
    workspace: scopedValue(workspace),
    nodeModels: {},
    edgeModels: {},
    containers: {},
    layout: {}
  };
}

export function visualProjectionOverlayKey(profileId: string, workspace: string): string {
  return `${scopedValue(profileId)}::${scopedValue(workspace)}`;
}

export function readVisualProjectionOverlay(
  storage: StorageLike | undefined,
  profileId: string,
  workspace: string
): VisualProjectionOverlay {
  const fallback = emptyVisualProjectionOverlay(profileId, workspace);
  const raw = storage?.getItem(VISUAL_OVERLAY_STORAGE_KEY);
  if (!raw) {
    return fallback;
  }
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    return sanitizeVisualProjectionOverlay(parsed[visualProjectionOverlayKey(profileId, workspace)], profileId, workspace);
  } catch {
    return fallback;
  }
}

export function writeVisualProjectionOverlay(storage: StorageLike | undefined, overlay: VisualProjectionOverlay) {
  if (!storage) {
    return;
  }
  let parsed: Record<string, unknown> = {};
  const raw = storage.getItem(VISUAL_OVERLAY_STORAGE_KEY);
  if (raw) {
    try {
      parsed = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      parsed = {};
    }
  }
  parsed[visualProjectionOverlayKey(overlay.profileId, overlay.workspace)] = sanitizeVisualProjectionOverlay(
    overlay,
    overlay.profileId,
    overlay.workspace
  );
  storage.setItem(VISUAL_OVERLAY_STORAGE_KEY, JSON.stringify(parsed));
}

export function sanitizeVisualProjectionOverlay(
  value: unknown,
  profileId: string,
  workspace: string
): VisualProjectionOverlay {
  if (!value || typeof value !== "object") {
    return emptyVisualProjectionOverlay(profileId, workspace);
  }
  const record = value as Partial<VisualProjectionOverlay>;
  return {
    version: 1,
    profileId: scopedValue(record.profileId ?? profileId),
    workspace: scopedValue(record.workspace ?? workspace),
    nodeModels: sanitizeNodeOverlays(record.nodeModels),
    edgeModels: sanitizeEdgeOverlays(record.edgeModels),
    containers: sanitizeContainerOverlays(record.containers),
    layout: sanitizeLayoutOverlay(record.layout)
  };
}

export function overlayHasOverrides(overlay: VisualProjectionOverlay): boolean {
  return Object.keys(overlay.nodeModels).length > 0
    || Object.keys(overlay.edgeModels).length > 0
    || Object.keys(overlay.containers).length > 0
    || Object.keys(definedOverlayValues(overlay.layout)).length > 0;
}

export function applyVisualProjectionOverlay(
  projection: FlightDeckVisualProjection | null,
  overlay: VisualProjectionOverlay
): FlightDeckVisualProjection | null {
  if (!projection || !overlayHasOverrides(overlay)) {
    return projection;
  }
  return {
    ...projection,
    nodeModels: projection.nodeModels.map((model) => ({
      ...model,
      ...definedOverlayValues(overlay.nodeModels[model.model])
    })),
    edgeModels: projection.edgeModels.map((model) => ({
      ...model,
      ...definedOverlayValues(overlay.edgeModels[model.model])
    })),
    dataLayout: {
      ...projection.dataLayout,
      layoutToken: overlay.layout.mode === "container-map" ? "container-map-local" : projection.dataLayout.layoutToken,
      groupBy: overlay.layout.mode === "container-map" ? "local-visual-containers" : projection.dataLayout.groupBy
    }
  };
}

function sanitizeNodeOverlays(value: unknown): Record<string, VisualProjectionNodeOverlay> {
  return sanitizeRecord(value, (overlay) => definedOverlayValues({
    label: cleanText(overlay.label, 80),
    colorToken: cleanToken(overlay.colorToken),
    group: cleanText(overlay.group, 80),
    glyph: cleanToken(overlay.glyph),
    shape: enumValue(overlay.shape, ["generated", "dot", "card", "hex", "diamond", "lane"]),
    detailDensity: enumValue(overlay.detailDensity, ["generated", "compact", "standard", "rich"]),
    visualRole: enumValue(overlay.visualRole, ["generated", "anchor", "context", "evidence", "decision", "risk", "actor", "object", "process"])
  }));
}

function sanitizeEdgeOverlays(value: unknown): Record<string, VisualProjectionEdgeOverlay> {
  return sanitizeRecord(value, (overlay) => definedOverlayValues({
    label: cleanText(overlay.label, 80),
    styleToken: enumValue(overlay.styleToken, ["generated", "directed", "dependency", "evidence", "warning"]),
    directionEmphasis: enumValue(overlay.directionEmphasis, ["generated", "normal", "strong", "muted"]),
    lineWeight: enumValue(overlay.lineWeight, ["generated", "fine", "normal", "strong"]),
    lineStyle: enumValue(overlay.lineStyle, ["generated", "solid", "dashed", "dotted"]),
    labelVisibility: enumValue(overlay.labelVisibility, ["generated", "hidden", "self-loops", "always"])
  }));
}

function sanitizeContainerOverlays(value: unknown): Record<string, VisualProjectionContainerOverlay> {
  return sanitizeRecord(value, (overlay) => definedOverlayValues({
    parentModel: cleanText(overlay.parentModel, 80),
    childModel: cleanText(overlay.childModel, 80),
    viaEdgeModel: cleanText(overlay.viaEdgeModel, 80),
    renderAs: enumValue(overlay.renderAs, ["generated", "region", "card", "lane", "section"]),
    collapse: enumValue(overlay.collapse, ["generated", "expanded", "collapsed"]),
    styleToken: cleanToken(overlay.styleToken)
  }));
}

function sanitizeLayoutOverlay(value: unknown): VisualProjectionLayoutOverlay {
  if (!value || typeof value !== "object") {
    return {};
  }
  const overlay = value as VisualProjectionLayoutOverlay;
  return definedOverlayValues({
    mode: enumValue(overlay.mode, ["edge-network", "container-map"]),
    style: enumValue(overlay.style, ["generated", "force", "groups", "hierarchy", "grid"])
  });
}

function sanitizeRecord<T extends object>(
  value: unknown,
  sanitize: (overlay: T) => T
): Record<string, T> {
  if (!value || typeof value !== "object") {
    return {};
  }
  const output: Record<string, T> = {};
  for (const [model, rawOverlay] of Object.entries(value as Record<string, unknown>)) {
    if (!rawOverlay || typeof rawOverlay !== "object") {
      continue;
    }
    const sanitized = sanitize(rawOverlay as T) as T;
    if (Object.keys(sanitized).length > 0) {
      output[cleanText(model, 80)] = sanitized;
    }
  }
  return output;
}

function definedOverlayValues<T extends object>(overlay: T | undefined): T {
  if (!overlay) {
    return {} as T;
  }
  return Object.fromEntries(
    Object.entries(overlay as Record<string, unknown>).filter(([, value]) => typeof value === "string" && value.trim() !== "" && value !== "generated")
  ) as T;
}

function enumValue<T extends string>(value: unknown, allowed: T[]): T | "" {
  const text = cleanText(value, 64) as T;
  return allowed.includes(text) ? text : "";
}

function scopedValue(value: unknown): string {
  return cleanText(value, 120) || "local-workspace";
}

function cleanText(value: unknown, maxLength: number): string {
  return String(value ?? "").replace(/[\u0000-\u001f\u007f]/g, "").trim().slice(0, maxLength);
}

function cleanToken(value: unknown): string {
  const token = cleanText(value, 64);
  return /^[a-z0-9-]+$/.test(token) ? token : "";
}
