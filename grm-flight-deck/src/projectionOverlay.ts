import type { FlightDeckVisualProjection } from "./types";

export const VISUAL_OVERLAY_STORAGE_KEY = "grm-flight-deck.visual-overlays.v1";

export interface VisualProjectionModelOverlay {
  label?: string;
  colorToken?: string;
  group?: string;
}

export interface VisualProjectionOverlay {
  version: 1;
  profileId: string;
  workspace: string;
  nodeModels: Record<string, VisualProjectionModelOverlay>;
  edgeModels: Record<string, VisualProjectionModelOverlay>;
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
    edgeModels: {}
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
    nodeModels: sanitizeModelOverlays(record.nodeModels),
    edgeModels: sanitizeModelOverlays(record.edgeModels)
  };
}

export function overlayHasOverrides(overlay: VisualProjectionOverlay): boolean {
  return Object.keys(overlay.nodeModels).length > 0 || Object.keys(overlay.edgeModels).length > 0;
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
    }))
  };
}

function sanitizeModelOverlays(value: unknown): Record<string, VisualProjectionModelOverlay> {
  if (!value || typeof value !== "object") {
    return {};
  }
  const output: Record<string, VisualProjectionModelOverlay> = {};
  for (const [model, rawOverlay] of Object.entries(value as Record<string, unknown>)) {
    if (!rawOverlay || typeof rawOverlay !== "object") {
      continue;
    }
    const overlay = rawOverlay as VisualProjectionModelOverlay;
    const sanitized = definedOverlayValues({
      label: cleanText(overlay.label, 80),
      colorToken: cleanToken(overlay.colorToken),
      group: cleanText(overlay.group, 80)
    });
    if (Object.keys(sanitized).length > 0) {
      output[cleanText(model, 80)] = sanitized;
    }
  }
  return output;
}

function definedOverlayValues(overlay: VisualProjectionModelOverlay | undefined): VisualProjectionModelOverlay {
  if (!overlay) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(overlay).filter(([, value]) => typeof value === "string" && value.trim() !== "")
  ) as VisualProjectionModelOverlay;
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
