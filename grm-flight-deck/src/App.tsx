import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { executeQueryCommand, fetchSecurityAuditStatus, fetchSecurityStatus, fetchSnapshot, fetchVisualProjection } from "./api";
import {
  createFlightDeckGraphStore,
  DEFAULT_GRAPH_FILTER,
  useFlightDeckGraphStore
} from "./graphStore";
import { GraphCanvas } from "./GraphCanvas";
import { colorForToken } from "./modelColors";
import {
  applyVisualProjectionOverlay,
  emptyVisualProjectionOverlay,
  overlayHasOverrides,
  readVisualProjectionOverlay,
  writeVisualProjectionOverlay,
  type VisualProjectionOverlay
} from "./projectionOverlay";
import type {
  FlightDeckSecurityAuditStatus,
  FlightDeckSecurityStatus,
  FlightDeckSnapshot,
  FlightDeckVisualProjection,
  GraphFilter,
  GraphView,
  QueryEvidence,
  QueryExecutionContext,
  SelectedGraphItem
} from "./types";

const graphStore = createFlightDeckGraphStore(window.localStorage);

const PROJECTION_COLOR_OPTIONS = [
  "flight-deck-blue",
  "flight-deck-teal",
  "flight-deck-indigo",
  "flight-deck-green",
  "flight-deck-violet",
  "flight-deck-amber",
  "flight-deck-rose",
  "flight-deck-sky"
];

function SchemaList({ title, models, projection }: { title: string; models: string[]; projection: FlightDeckVisualProjection | null }) {
  return (
    <section className="schema-list-section">
      <h3>{title}</h3>
      <ul className="model-list">
        {models.map((model) => (
          <li key={model}>
            <span
              className="model-swatch"
              style={{ backgroundColor: colorForToken(nodeProjection(projection, model)?.colorToken, model) }}
            />
            <span>{nodeProjection(projection, model)?.label ?? edgeProjection(projection, model)?.label ?? model}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function ProjectionPanel({
  projection,
  snapshot,
  graphView
}: {
  projection: FlightDeckVisualProjection | null;
  snapshot: FlightDeckSnapshot | null;
  graphView: GraphView;
}) {
  const activeLayout = graphView === "schema" ? projection?.schemaLayout : projection?.dataLayout;

  return (
    <section className={`projection-panel ${projection ? "available" : "missing"}`} aria-label="Visual projection">
      <div className="projection-panel-heading">
        <h2>Projection</h2>
        <span>{projection ? projectionSourceLabel(projection) : "fallback"}</span>
      </div>
      {projection ? (
        <>
          <p className="projection-note">Generated visual guidance; workspace data remains canonical.</p>
          <dl>
            <dt>From</dt>
            <dd>{projection.provenance.generatedFrom}</dd>
            <dt>Scope</dt>
            <dd>{projection.nodeModels.length} node models / {projection.edgeModels.length} edge models</dd>
            <dt>Layout</dt>
            <dd>{activeLayout?.layoutToken ?? "default"}</dd>
            <dt>Group by</dt>
            <dd>{activeLayout?.groupBy ?? "model"}</dd>
            <dt>Limit</dt>
            <dd>{projection.provenance.modelLimit}</dd>
          </dl>
        </>
      ) : (
        <p className="projection-note">
          No projection hints loaded; using snapshot labels, local colors, and default layouts.
        </p>
      )}
      {snapshot && (
        <p className="projection-footnote">
          Current snapshot: {snapshot.source}, {snapshot.nodes.length} nodes / {snapshot.edges.length} edges.
        </p>
      )}
    </section>
  );
}

function projectionSourceLabel(projection: FlightDeckVisualProjection): string {
  switch (projection.provenance.source) {
    case "generated_default":
      return "generated";
    case "fixture_default":
      return "fixture";
    default:
      return "unknown";
  }
}

function ProjectionOverlayControls({
  projection,
  overlay,
  onOverlayChange,
  onReset
}: {
  projection: FlightDeckVisualProjection | null;
  overlay: VisualProjectionOverlay;
  onOverlayChange: (overlay: VisualProjectionOverlay) => void;
  onReset: () => void;
}) {
  const [selectedModel, setSelectedModel] = useState("");
  const nodeModels = projection?.nodeModels ?? [];
  const activeModel = nodeModels.find((model) => model.model === selectedModel) ?? nodeModels[0] ?? null;
  const activeOverride = activeModel ? overlay.nodeModels[activeModel.model] ?? {} : {};

  useEffect(() => {
    if (!activeModel) {
      setSelectedModel("");
      return;
    }
    if (!nodeModels.some((model) => model.model === selectedModel)) {
      setSelectedModel(activeModel.model);
    }
  }, [activeModel, nodeModels, selectedModel]);

  const updateActiveOverride = (patch: { label?: string; colorToken?: string; group?: string }) => {
    if (!activeModel) {
      return;
    }
    const nextNodeModels = {
      ...overlay.nodeModels,
      [activeModel.model]: {
        ...activeOverride,
        ...patch
      }
    };
    const compacted = Object.fromEntries(
      Object.entries(nextNodeModels).filter(([, value]) =>
        Object.values(value).some((item) => typeof item === "string" && item.trim() !== "")
      )
    );
    onOverlayChange({
      ...overlay,
      nodeModels: compacted
    });
  };

  return (
    <section className="projection-controls" aria-label="Local visual projection overrides">
      <div className="projection-panel-heading">
        <h2>Local overlays</h2>
        <span>{overlayHasOverrides(overlay) ? "custom" : "default"}</span>
      </div>
      <p className="projection-note">Browser-local advisory model styling for this profile and workspace.</p>
      <label>
        Model
        <select
          value={activeModel?.model ?? ""}
          onChange={(event) => setSelectedModel(event.target.value)}
          disabled={!activeModel}
        >
          {nodeModels.map((model) => (
            <option value={model.model} key={model.model}>{model.model}</option>
          ))}
        </select>
      </label>
      <label>
        Label
        <input
          value={activeOverride.label ?? ""}
          onChange={(event) => updateActiveOverride({ label: event.target.value })}
          placeholder={activeModel?.label ?? "generated label"}
          disabled={!activeModel}
        />
      </label>
      <label>
        Color
        <select
          value={activeOverride.colorToken ?? ""}
          onChange={(event) => updateActiveOverride({ colorToken: event.target.value })}
          disabled={!activeModel}
        >
          <option value="">Generated</option>
          {PROJECTION_COLOR_OPTIONS.map((token) => (
            <option value={token} key={token}>{token.replace("flight-deck-", "")}</option>
          ))}
        </select>
      </label>
      <label>
        Group
        <input
          value={activeOverride.group ?? ""}
          onChange={(event) => updateActiveOverride({ group: event.target.value })}
          placeholder={activeModel?.group ?? "generated group"}
          disabled={!activeModel}
        />
      </label>
      <button type="button" onClick={onReset} disabled={!overlayHasOverrides(overlay)}>
        Reset
      </button>
    </section>
  );
}

function SelectionPanel({
  selected,
  projection,
  open,
  onOpenChange
}: {
  selected: SelectedGraphItem | null;
  projection: FlightDeckVisualProjection | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  if (!selected || !open) {
    return (
      <aside className={`panel inspector collapsed ${selected ? "has-selection" : ""}`} aria-label="Selection">
        {selected && (
          <button
            type="button"
            className="panel-rail-button"
            onClick={() => onOpenChange(true)}
            aria-label="Open selection details"
          >
            &lt;
          </button>
        )}
      </aside>
    );
  }

  const hint = selected.kind === "node"
    ? nodeProjection(projection, selected.model)
    : edgeProjection(projection, selected.model);
  const orderedProps = orderProps(selected.props, hint?.detailFields ?? []);
  const entries = Object.entries(orderedProps);

  return (
    <aside className="panel inspector open" aria-label="Selection">
      <div className="panel-heading-row">
        <div>
          <h2>{selected.label}</h2>
          <p className="kind">{selected.kind} / {hint?.label ?? selected.model}</p>
        </div>
        <button
          type="button"
          className="icon-button"
          onClick={() => onOpenChange(false)}
          aria-label="Collapse selection details"
        >
          &gt;
        </button>
      </div>
      {entries.length > 0 ? (
        <dl className="selection-fields">
          {entries.map(([field, value]) => (
            <div key={field}>
              <dt>{field}</dt>
              <dd>{displayJsonValue(value)}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="muted">No properties are available for this item.</p>
      )}
    </aside>
  );
}

function EventStreamPanel({
  auditStatus,
  events
}: {
  auditStatus: FlightDeckSecurityAuditStatus | null;
  events: ReturnType<typeof graphStore.getState>["events"];
}) {
  const [auditFilter, setAuditFilter] = useState("");
  const needle = auditFilter.trim().toLowerCase();
  const serviceEvents = (auditStatus?.recentEvents ?? []).filter((event) => auditRecordMatches(event, needle));
  const localEvents = events.filter((event) => auditRecordMatches(event, needle));

  return (
    <section className="audit-panel" aria-label="Execution events">
      <div className="audit-toolbar">
        <div>
          <h2>Audit</h2>
          <p className="muted">Service-authored audit evidence and bounded local UI observations.</p>
        </div>
        <label className="audit-search">
          Audit search
          <input
            value={auditFilter}
            onChange={(event) => setAuditFilter(event.target.value)}
            placeholder="principal, workspace, outcome, operation"
          />
        </label>
      </div>
      {auditStatus && (
        <div className={`audit-status ${auditStatus.auditMode} ${auditStatus.sinkHealth}`}>
          <div>
            <span>{securityProfileLabel(auditStatus.securityProfile)}</span>
            <strong>{auditModeLabel(auditStatus.auditMode)}</strong>
          </div>
          <dl>
            <dt>Sink</dt>
            <dd>{auditStatus.sinkHealth}</dd>
            <dt>Mandatory</dt>
            <dd>{auditStatus.mandatoryAuditAvailable ? "available" : "not available"}</dd>
            <dt>Retained</dt>
            <dd>{auditStatus.retainedEventCount} events / max {auditStatus.retentionMaxEvents}</dd>
            <dt>Recovery</dt>
            <dd>{auditStatus.lastRecoveryStatusCode ?? "unknown"}</dd>
          </dl>
        </div>
      )}
      {serviceEvents.length ? (
        <ol className="audit-list service-audit-list">
          {serviceEvents.map((event) => (
            <li className={`audit-item ${event.decision}`} key={`${event.requestId}-${event.serviceSequence}`}>
              <div>
                <span>{event.stage}</span>
                <strong>{event.decision}</strong>
              </div>
              <p>{event.operationFamily ?? event.reasonCode ?? "service audit event"}</p>
              <dl>
                <dt>Principal</dt>
                <dd>{event.principal ? `${event.principal.issuer}/${event.principal.subject}` : "none"}</dd>
                <dt>Workspace</dt>
                <dd>{event.workspace ?? "service"}</dd>
                <dt>Outcomes</dt>
                <dd>{event.runtimeOutcome} / {event.durabilityOutcome} / {event.deliveryOutcome}</dd>
              </dl>
            </li>
          ))}
        </ol>
      ) : (
        <p className="muted">No service-authored audit events match this profile and filter.</p>
      )}
      <ol className="audit-list">
        {localEvents.map((event) => (
          <li className={`audit-item ${event.kind}`} key={event.id}>
            <div>
              <span>{event.kind}</span>
              <strong>{event.status}</strong>
            </div>
            <p>{event.operationSummary ?? event.label}</p>
            <dl>
              <dt>Context</dt>
              <dd>{event.securityContext ?? "local UI event buffer"}</dd>
              <dt>Workspace</dt>
              <dd>{event.workspace ?? "not loaded"}</dd>
              <dt>Outcome</dt>
              <dd>{event.resultState ?? event.label}</dd>
            </dl>
          </li>
        ))}
      </ol>
    </section>
  );
}


function auditRecordMatches(record: unknown, needle: string): boolean {
  if (!needle) {
    return true;
  }
  return String(JSON.stringify(record) ?? "").toLowerCase().includes(needle);
}


function nodeProjection(projection: FlightDeckVisualProjection | null, model: string) {
  return projection?.nodeModels.find((hint) => hint.model === model) ?? null;
}

function edgeProjection(projection: FlightDeckVisualProjection | null, model: string) {
  return projection?.edgeModels.find((hint) => hint.model === model) ?? null;
}

function orderProps(props: SelectedGraphItem["props"], detailFields: string[]) {
  const ordered: SelectedGraphItem["props"] = {};
  for (const field of detailFields) {
    if (Object.prototype.hasOwnProperty.call(props, field)) {
      ordered[field] = props[field];
    }
  }
  for (const [key, value] of Object.entries(props)) {
    if (!Object.prototype.hasOwnProperty.call(ordered, key)) {
      ordered[key] = value;
    }
  }
  return ordered;
}

function displayJsonValue(value: SelectedGraphItem["props"][string]): string {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value) || typeof value === "object") {
    return JSON.stringify(value);
  }
  return String(value);
}

function auditModeLabel(mode: FlightDeckSecurityAuditStatus["auditMode"]): string {
  switch (mode) {
    case "mandatory":
      return "Mandatory audit";
    case "best_effort":
      return "Best-effort audit";
    case "unavailable":
      return "Audit unavailable";
    case "not_applicable":
      return "Audit not applicable";
    default:
      return "Audit unknown";
  }
}

function QueryInsightPanels({
  explainVisible,
  profileVisible,
  snapshot,
  visibleSnapshot,
  filter,
  graphView,
  lastExecutedQuery,
  queryEvidence
}: {
  explainVisible: boolean;
  profileVisible: boolean;
  snapshot: FlightDeckSnapshot | null;
  visibleSnapshot: FlightDeckSnapshot | null;
  filter: GraphFilter;
  graphView: GraphView;
  lastExecutedQuery: QueryExecutionContext | null;
  queryEvidence: QueryEvidence | null;
}) {
  if (!explainVisible && !profileVisible) {
    return null;
  }

  const serviceRows = metricNumber(queryEvidence?.rowCount);
  const elapsedMicros = metricNumber(queryEvidence?.elapsedMicros);

  return (
    <aside className="query-insights" aria-label="Query explain and profile">
      {explainVisible && (
        <section className="insight-panel">
          <h2>Explain</h2>
          <p className="muted">{evidenceLabel(queryEvidence, "explain")}</p>
          <dl>
            <dt>Operation</dt>
            <dd>{lastExecutedQuery?.queryShape ?? (graphView === "schema" ? "schema projection" : "bounded snapshot filter")}</dd>
            <dt>Workspace</dt>
            <dd>{lastExecutedQuery?.workspace ?? snapshot?.workspace ?? "not loaded"}</dd>
            <dt>Source</dt>
            <dd>{sourceSummary(snapshot, queryEvidence)}</dd>
            <dt>Model scope</dt>
            <dd>{graphView === "schema" ? "schema catalogue" : filter.model || "all models"}</dd>
            <dt>Predicates</dt>
            <dd>{lastExecutedQuery?.commandText || (graphView === "schema" ? "not applied to schema view" : predicateSummary(filter))}</dd>
            <dt>Result shape</dt>
            <dd>{resultShapeSummary(snapshot, visibleSnapshot, graphView, lastExecutedQuery)}</dd>
            <dt>Capability</dt>
            <dd>{queryEvidence?.provenance === "service" ? queryEvidence.planKind ?? "service evidence" : graphView === "schema" ? "local schema projection" : "local summary only"}</dd>
            {queryEvidence?.steps?.length ? (
              <>
                <dt>Steps</dt>
                <dd>{queryEvidence.steps.join(" / ")}</dd>
              </>
            ) : null}
            {queryEvidence?.indexes?.length ? (
              <>
                <dt>Indexes</dt>
                <dd>{queryEvidence.indexes.join(", ")}</dd>
              </>
            ) : null}
          </dl>
        </section>
      )}
      {profileVisible && (
        <section className="insight-panel">
          <h2>Profile</h2>
          <p className="muted">{evidenceLabel(queryEvidence, "profile")}</p>
          <dl>
            <dt>Source rows</dt>
            <dd>{sourceRowsSummary(snapshot, graphView, lastExecutedQuery)}</dd>
            <dt>Visible rows</dt>
            <dd>{resultShapeSummary(snapshot, visibleSnapshot, graphView, lastExecutedQuery)}</dd>
            {queryEvidence?.provenance === "service" && (
              <>
                <dt>Service rows</dt>
                <dd>{serviceRows ?? "not reported"}</dd>
                <dt>Elapsed</dt>
                <dd>{elapsedMicros === null ? "not reported" : `${elapsedMicros} us`}</dd>
              </>
            )}
            <dt>Limit</dt>
            <dd>{lastExecutedQuery?.limit ?? snapshot?.modelLimit ?? "none"}</dd>
            <dt>Omitted edges</dt>
            <dd>{lastExecutedQuery?.omittedEdges ?? visibleSnapshot?.omittedEdges ?? 0}</dd>
            <dt>Partial</dt>
            <dd>{lastExecutedQuery?.partialReason ?? visibleSnapshot?.partialReason ? "yes" : "no"}</dd>
          </dl>
        </section>
      )}
    </aside>
  );
}

function metricNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function evidenceLabel(evidence: QueryEvidence | null, panel: "explain" | "profile"): string {
  if (evidence?.provenance === "service") {
    return `Service/runtime ${panel} evidence.`;
  }
  if (evidence?.provenance === "unsupported") {
    return evidence.unsupportedReason ?? "Unsupported query shape.";
  }
  return panel === "explain"
    ? "Local view summary, not a service planner result."
    : "Local row counts from the current visible snapshot.";
}

function sourceSummary(snapshot: FlightDeckSnapshot | null, evidence: QueryEvidence | null): string {
  if (evidence?.provenance === "service") {
    return "typed service/runtime query";
  }
  return snapshot?.source === "service" ? "service snapshot, client-side summary" : "fixture/client-side summary";
}

function SecurityStatusPanel({ status }: { status: FlightDeckSecurityStatus | null }) {
  if (!status) {
    return null;
  }

  return (
    <span className={`security-status ${status.securityProfile}`}>
      <strong>{securityProfileLabel(status.securityProfile)}</strong>
      <span>{identityLabel(status)}</span>
      {status.policyVersion && <span>{status.policyVersion}</span>}
    </span>
  );
}

function securityProfileLabel(profile: FlightDeckSecurityStatus["securityProfile"]): string {
  switch (profile) {
    case "anonymous_local":
      return "Anonymous local";
    case "docker_local_insecure":
      return "Docker local insecure";
    case "secured":
      return "Secured";
    case "fixture":
      return "Fixture";
    default:
      return "Unknown";
  }
}

function identityLabel(status: FlightDeckSecurityStatus): string {
  if (status.principal) {
    const method = status.authenticationMethod ? ` via ${status.authenticationMethod}` : "";
    return `${status.principal.issuer}/${status.principal.subject}${method}`;
  }
  switch (status.identityStatus) {
    case "anonymous_local":
      return "anonymous local";
    case "docker_local_insecure":
      return "no application principal";
    case "fixture":
      return "fixture data";
    default:
      return "no authenticated principal";
  }
}

function predicateSummary(filter: GraphFilter): string {
  const predicates = [
    filter.text.trim() ? `text contains ${filter.text.trim()}` : "",
    filter.propertyKey.trim() ? `${filter.propertyKey.trim()} contains ${filter.propertyValue.trim() || "*"}` : ""
  ].filter(Boolean);

  return predicates.length > 0 ? predicates.join(", ") : "none";
}

function resultShapeSummary(
  snapshot: FlightDeckSnapshot | null,
  visibleSnapshot: FlightDeckSnapshot | null,
  graphView: GraphView,
  lastExecutedQuery: QueryExecutionContext | null
): string {
  if (lastExecutedQuery) {
    return graphView === "schema"
      ? `${lastExecutedQuery.visibleNodes} models / ${lastExecutedQuery.visibleEdges} schema edges`
      : `${lastExecutedQuery.visibleNodes} nodes / ${lastExecutedQuery.visibleEdges} edges`;
  }
  if (!snapshot) {
    return "none";
  }
  if (graphView === "schema") {
    return `${snapshot.nodeModels.length} models / ${schemaEdgeCount(snapshot)} schema edges`;
  }
  return visibleSnapshot ? `${visibleSnapshot.nodes.length} nodes / ${visibleSnapshot.edges.length} edges` : "none";
}

function sourceRowsSummary(
  snapshot: FlightDeckSnapshot | null,
  graphView: GraphView,
  lastExecutedQuery: QueryExecutionContext | null
): string {
  if (lastExecutedQuery) {
    return graphView === "schema"
      ? `${lastExecutedQuery.sourceNodes} models / ${lastExecutedQuery.sourceEdges} schema edges`
      : `${lastExecutedQuery.sourceNodes} nodes / ${lastExecutedQuery.sourceEdges} edges`;
  }
  if (!snapshot) {
    return "none";
  }
  return graphView === "schema"
    ? `${snapshot.nodeModels.length} models / ${schemaEdgeCount(snapshot)} schema edges`
    : `${snapshot.nodes.length} nodes / ${snapshot.edges.length} edges`;
}

function schemaEdgeCount(snapshot: FlightDeckSnapshot): number {
  return snapshot.schemaEdges?.length ?? snapshot.edgeModels.length;
}

function schemaFilteredSnapshot(
  snapshot: FlightDeckSnapshot | null,
  filter: string
): FlightDeckSnapshot | null {
  if (!snapshot) {
    return null;
  }
  const needle = filter.trim().toLowerCase();
  if (!needle) {
    return snapshot;
  }

  const schemaNodeModels = snapshot.schemaNodeModels ?? snapshot.nodeModels.map((name) => ({
    name,
    idField: "id",
    fields: []
  }));
  const schemaEdgeModels = snapshot.schemaEdgeModels ?? snapshot.schemaEdges?.map((edge) => ({
    name: edge.model,
    fromModel: edge.fromModel,
    toModel: edge.toModel,
    idField: "id",
    fields: []
  })) ?? [];
  const matchingNodeNames = new Set(
    schemaNodeModels
      .filter((model) => schemaModelMatches(`${model.name} ${model.idField}`, model.fields, needle))
      .map((model) => model.name)
  );
  const matchingEdgeModels = schemaEdgeModels.filter((model) =>
    schemaModelMatches(`${model.name} ${model.fromModel} ${model.toModel} ${model.idField}`, model.fields, needle)
  );

  for (const edge of matchingEdgeModels) {
    matchingNodeNames.add(edge.fromModel);
    matchingNodeNames.add(edge.toModel);
  }

  const schemaEdges = (snapshot.schemaEdges ?? matchingEdgeModels.map((model) => ({
    model: model.name,
    fromModel: model.fromModel,
    toModel: model.toModel
  }))).filter((edge) =>
    matchingNodeNames.has(edge.fromModel) &&
    matchingNodeNames.has(edge.toModel) &&
    (matchingEdgeModels.some((model) => model.name === edge.model) ||
      edge.model.toLowerCase().includes(needle) ||
      edge.fromModel.toLowerCase().includes(needle) ||
      edge.toModel.toLowerCase().includes(needle))
  );

  return {
    ...snapshot,
    nodeModels: schemaNodeModels
      .filter((model) => matchingNodeNames.has(model.name))
      .map((model) => model.name),
    edgeModels: schemaEdges.map((edge) => edge.model),
    schemaNodeModels: schemaNodeModels.filter((model) => matchingNodeNames.has(model.name)),
    schemaEdgeModels: schemaEdgeModels.filter((model) =>
      schemaEdges.some((edge) => edge.model === model.name && edge.fromModel === model.fromModel && edge.toModel === model.toModel)
    ),
    schemaEdges
  };
}

function schemaModelMatches(
  text: string,
  fields: { name: string; valueType: string; required: boolean }[],
  needle: string
): boolean {
  return `${text} ${fields.map((field) => `${field.name} ${field.valueType}`).join(" ")}`
    .toLowerCase()
    .includes(needle);
}

function sessionCommandFor(mode: "explain" | "profile", command: string): string {
  const baseCommand = command.trim().replace(/^session\.(?:explain|profile)\s+/i, "");
  return `session.${mode} ${baseCommand}`;
}

function connectionKindLabel(
  settings: ReturnType<typeof graphStore.getState>["settings"],
  status: FlightDeckSecurityStatus | null
): string {
  if (settings.useFixtureData) {
    return "fixture";
  }
  if (status && status.securityProfile !== "unknown") {
    return securityProfileLabel(status.securityProfile).toLowerCase();
  }
  if (status?.securityProfile === "unknown") {
    return "status unknown";
  }

  return `configured ${configuredConnectionKindLabel(settings)}`;
}

function configuredConnectionKindLabel(settings: ReturnType<typeof graphStore.getState>["settings"]): string {
  switch (settings.mode) {
    case "anonymous_local":
    case "local-anonymous-dev":
      return "anonymous local";
    case "docker_local_insecure":
      return "docker local insecure";
    case "secured":
      return "secured";
    default:
      return "local gateway";
  }
}

export function App() {
  const storeState = useFlightDeckGraphStore(graphStore);
  const [securityStatus, setSecurityStatus] = useState<FlightDeckSecurityStatus | null>(null);
  const [securityAuditStatus, setSecurityAuditStatus] = useState<FlightDeckSecurityAuditStatus | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [queryLoading, setQueryLoading] = useState(false);
  const [schemaPanelOpen, setSchemaPanelOpen] = useState(false);
  const [selectionPanelOpen, setSelectionPanelOpen] = useState(false);
  const [projectionOverlay, setProjectionOverlay] = useState(() =>
    emptyVisualProjectionOverlay(graphStore.getState().selectedProfileId, graphStore.getState().settings.workspace)
  );
  const fixtureAutoLoaded = useRef(false);

  const {
    profiles,
    selectedProfileId,
    draftProfileName,
    settings,
    filter,
    snapshot,
    visibleSnapshot,
    selectedItem,
    status,
    statusDetail,
    lastError,
    events,
    activeWorkspacePanel,
    graphView,
    connectionDetailsOpen,
    explainVisible,
    profileVisible,
    lastExecutedQuery,
    queryCommand,
    queryStatus,
    queryMessage,
    queryEvidence,
    schemaFilter,
    visualProjection
  } = storeState;

  const modelOptions = useMemo(() => {
    if (!snapshot) {
      return [];
    }
    return [...snapshot.nodeModels, ...snapshot.edgeModels].sort();
  }, [snapshot]);
  const schemaGraphSnapshot = useMemo(
    () => schemaFilteredSnapshot(snapshot, schemaFilter),
    [snapshot, schemaFilter]
  );
  const effectiveVisualProjection = useMemo(
    () => applyVisualProjectionOverlay(visualProjection, projectionOverlay),
    [projectionOverlay, visualProjection]
  );
  const graphSnapshot = graphView === "schema" ? schemaGraphSnapshot : visibleSnapshot;
  const workspaceMode = activeWorkspacePanel === "audit" ? "audit" : graphView;
  const insightPanelsVisible = graphView === "data" && (explainVisible || profileVisible);

  useEffect(() => {
    setProjectionOverlay(readVisualProjectionOverlay(window.localStorage, selectedProfileId, settings.workspace));
  }, [selectedProfileId, settings.workspace]);

  useEffect(() => {
    writeVisualProjectionOverlay(window.localStorage, projectionOverlay);
  }, [projectionOverlay]);

  const selectWorkspaceMode = (mode: "data" | "schema" | "audit") => {
    if (mode === "audit") {
      graphStore.selectWorkspacePanel("audit");
      setSchemaPanelOpen(false);
      return;
    }
    graphStore.selectWorkspacePanel("query");
    graphStore.setGraphView(mode);
    setSchemaPanelOpen(mode === "schema");
  };

  const collapseSchemaPanel = () => {
    setSchemaPanelOpen(false);
    if (graphView === "schema") {
      graphStore.setGraphView("data");
    }
  };

  const load = async (event?: FormEvent) => {
    event?.preventDefault();
    setLoading(true);
    graphStore.beginSnapshotLoad(settings.useFixtureData);

    const controller = new AbortController();
    try {
      try {
        const loadedSecurityStatus = await fetchSecurityStatus(settings, controller.signal);
        setSecurityStatus(loadedSecurityStatus);
      } catch {
        setSecurityStatus({
          securityProfile: "unknown",
          identityStatus: "unknown",
          principal: null,
          authenticationMethod: null,
          policyVersion: null
        });
      }
      try {
        const loadedAuditStatus = await fetchSecurityAuditStatus(settings, controller.signal);
        setSecurityAuditStatus(loadedAuditStatus);
      } catch {
        setSecurityAuditStatus({
          securityProfile: settings.useFixtureData ? "fixture" : "unknown",
          auditMode: "unavailable",
          sinkHealth: "unavailable",
          mandatoryAuditAvailable: false,
          retainedEventCount: 0,
          recentEventCount: 0,
          retentionMaxEvents: 0,
          retentionMaxBytes: 0,
          retentionMaxAgeSeconds: 0,
          futureDatedRecordCount: 0,
          lastRecoveryStatusCode: "gateway_or_service_unavailable",
          recentEvents: []
        });
      }
      try {
        graphStore.loadVisualProjection(await fetchVisualProjection(settings, controller.signal));
      } catch {
        graphStore.loadVisualProjection(null);
      }
      const loaded = await fetchSnapshot(settings, controller.signal);
      graphStore.loadSnapshot(loaded);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSecurityStatus(null);
      setSecurityAuditStatus(null);
      graphStore.markConnectionFailed(message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (fixtureAutoLoaded.current || !settings.useFixtureData || snapshot || loading) {
      return;
    }

    fixtureAutoLoaded.current = true;
    void load();
  }, [settings.useFixtureData, snapshot, loading]);

  useEffect(() => {
    setSelectionPanelOpen(Boolean(selectedItem));
  }, [selectedItem]);

  const handleSelect = useCallback((item: SelectedGraphItem | null) => {
    graphStore.selectGraphItem(item ? { kind: item.kind, id: item.id } : null);
  }, []);

  const executeQuery = async (mode: "query" | "explain" | "profile" = "query") => {
    if (graphView === "schema") {
      graphStore.recordQueryExecution();
      return;
    }
    if (settings.useFixtureData) {
      graphStore.rejectQueryCommand("Typed query execution requires a service connection; fixture mode can use the local filter summary.");
      return;
    }
    const command = mode === "query" ? queryCommand : sessionCommandFor(mode, queryCommand);
    setQueryLoading(true);
    graphStore.beginQueryCommand();
    const controller = new AbortController();
    try {
      const response = await executeQueryCommand(settings, command, controller.signal);
      graphStore.applyQueryResponse(response);
      graphStore.setExplainVisible(response.kind === "explain");
      graphStore.setProfileVisible(response.kind === "profile");
    } catch (error) {
      graphStore.failQueryCommand(error instanceof Error ? error.message : String(error));
    } finally {
      setQueryLoading(false);
    }
  };

  return (
    <main>
      <header className="topbar">
        <div>
          <p className="eyebrow">First service UI</p>
          <h1>GRM flight-deck</h1>
        </div>
        <div className="connection-summary">
          <span className="connection-summary-text">
            <strong>{profiles.find((profile) => profile.id === selectedProfileId)?.name ?? "Local workspace"}</strong>
            {connectionKindLabel(settings, securityStatus)}
            {" / "}
            {settings.workspace}
          </span>
          <SecurityStatusPanel status={securityStatus} />
          <button
            type="button"
            className="secondary-button"
            onClick={() => graphStore.setConnectionDetailsOpen(!connectionDetailsOpen)}
            aria-expanded={connectionDetailsOpen}
          >
            {connectionDetailsOpen ? "Hide connection" : "Change connection"}
          </button>
          <button type="button" onClick={() => void load()} disabled={loading || settings.workspace.trim() === ""}>
            {loading ? "Loading" : "Connect"}
          </button>
        </div>
        {connectionDetailsOpen && (
          <div className="modal-backdrop" role="presentation" onMouseDown={() => graphStore.setConnectionDetailsOpen(false)}>
            <section
              className="connection-modal"
              role="dialog"
              aria-modal="true"
              aria-label="Connection settings"
              onMouseDown={(event) => event.stopPropagation()}
            >
              <div className="modal-heading-row">
                <div>
                  <h2>Connection</h2>
                  <p className="muted">Safe browser-facing profile settings for the local gateway.</p>
                </div>
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => graphStore.setConnectionDetailsOpen(false)}
                  aria-label="Close connection settings"
                >
                  X
                </button>
              </div>
              <form className="connection-form modal-form" onSubmit={load}>
                <label>
                  Profile
                  <select
                    value={selectedProfileId}
                    onChange={(event) => graphStore.selectProfile(event.target.value)}
                  >
                    {profiles.map((profile) => (
                      <option value={profile.id} key={profile.id}>{profile.name}</option>
                    ))}
                  </select>
                </label>
                <label>
                  Profile name
                  <input
                    value={draftProfileName}
                    onChange={(event) => graphStore.updateDraftProfileName(event.target.value)}
                  />
                </label>
                <label>
                  Gateway URL
                  <input
                    placeholder="blank uses /api proxy, or http://127.0.0.1:3001"
                    value={settings.serviceBaseUrl}
                    onChange={(event) =>
                      graphStore.updateSettings({ serviceBaseUrl: event.target.value })
                    }
                    disabled={settings.useFixtureData}
                  />
                </label>
                <label>
                  Workspace
                  <input
                    value={settings.workspace}
                    onChange={(event) =>
                      graphStore.updateSettings({ workspace: event.target.value })
                    }
                  />
                </label>
                <label>
                  Connection kind
                  <select
                    value={settings.mode}
                    onChange={(event) =>
                      graphStore.updateSettings({ mode: event.target.value as typeof settings.mode })
                    }
                    disabled={settings.useFixtureData}
                  >
                    <option value="local-anonymous-dev">Local anonymous dev</option>
                    <option value="anonymous_local">Anonymous local</option>
                    <option value="docker_local_insecure">Docker local insecure</option>
                    <option value="secured">Secured</option>
                  </select>
                </label>
                <label>
                  Limit
                  <input
                    type="number"
                    min="1"
                    max="25000"
                    value={settings.limit}
                    onChange={(event) =>
                      graphStore.updateSettings({ limit: Number(event.target.value) })
                    }
                  />
                </label>
                <label className="check-label">
                  <input
                    type="checkbox"
                    checked={settings.useFixtureData}
                    onChange={(event) =>
                      graphStore.updateSettings({ useFixtureData: event.target.checked })
                    }
                  />
                  Fixture
                </label>
                <div className="modal-actions">
                  <button type="button" onClick={graphStore.saveCurrentProfile}>
                    Save profile
                  </button>
                  <button type="button" onClick={graphStore.createProfile}>
                    New profile
                  </button>
                  <button type="submit" disabled={loading || settings.workspace.trim() === ""}>
                    {loading ? "Loading" : "Connect"}
                  </button>
                </div>
              </form>
            </section>
          </div>
        )}
      </header>

      <section className="status-line">
        <span className="status-item">
          <span>{status}</span>
          {statusDetail && <span>{statusDetail}</span>}
        </span>
        {hovered && <span className="hover-preview">{hovered}</span>}
        {lastError && <span className="error">{lastError}</span>}
      </section>

      <section className="workspace-nav" aria-label="Workspace mode">
        <button
          type="button"
          className={workspaceMode === "data" ? "active" : ""}
          onClick={() => selectWorkspaceMode("data")}
        >
          Data
        </button>
        <button
          type="button"
          className={workspaceMode === "schema" ? "active" : ""}
          onClick={() => selectWorkspaceMode("schema")}
        >
          Schema
        </button>
        <button
          type="button"
          className={workspaceMode === "audit" ? "active" : ""}
          onClick={() => selectWorkspaceMode("audit")}
        >
          Audit
        </button>
      </section>

      <div className={`flight-deck ${schemaPanelOpen ? "schema-open" : "schema-collapsed"} ${selectionPanelOpen && selectedItem ? "selection-open" : "selection-collapsed"}`}>
        {schemaPanelOpen && (
          <aside className="panel schema-panel open" aria-label="Schema and projection">
            <div className="panel-heading-row">
              <h2>Schema</h2>
              <button
                type="button"
                className="icon-button"
                onClick={collapseSchemaPanel}
                aria-label="Collapse schema panel"
              >
                &lt;
              </button>
            </div>
            <SchemaList title="Node models" models={visibleSnapshot?.nodeModels ?? []} projection={effectiveVisualProjection} />
            <SchemaList title="Edge models" models={visibleSnapshot?.edgeModels ?? []} projection={effectiveVisualProjection} />
            <ProjectionPanel projection={effectiveVisualProjection} snapshot={snapshot} graphView={graphView} />
            <ProjectionOverlayControls
              projection={visualProjection}
              overlay={projectionOverlay}
              onOverlayChange={setProjectionOverlay}
              onReset={() => setProjectionOverlay(emptyVisualProjectionOverlay(selectedProfileId, settings.workspace))}
            />
            {visibleSnapshot?.partialReason && (
              <p className="warning">{visibleSnapshot.partialReason}</p>
            )}
            {visibleSnapshot && visibleSnapshot.omittedEdges > 0 && (
              <p className="warning">
                {visibleSnapshot.omittedEdges} edges omitted outside the bounded node result.
              </p>
            )}
          </aside>
        )}

        <div className="graph-column">

          {activeWorkspacePanel === "query" ? (
            <>
              <section className="query-bar" aria-label={graphView === "schema" ? "Schema filter" : "Data query and filter"}>
                {graphView === "data" ? (
                  <>
                    <label className="command-input">
                      GRM command
                      <input
                        value={queryCommand}
                        onChange={(event) => graphStore.setQueryCommand(event.target.value)}
                        placeholder="node.find WorkSlice status=active limit=25"
                        disabled={!snapshot && settings.useFixtureData}
                      />
                    </label>
                    <label>
                      Search
                      <input
                        value={filter.text}
                        onChange={(event) => graphStore.applyGraphFilter({ ...filter, text: event.target.value })}
                        placeholder="id, label, model, property"
                        disabled={!snapshot}
                      />
                    </label>
                    <label>
                      Model
                      <select
                        value={filter.model}
                        onChange={(event) => graphStore.applyGraphFilter({ ...filter, model: event.target.value })}
                        disabled={!snapshot}
                      >
                        <option value="">Any</option>
                        {modelOptions.map((model) => (
                          <option value={model} key={model}>{model}</option>
                        ))}
                      </select>
                    </label>
                    <label>
                      Property key
                      <input
                        value={filter.propertyKey}
                        onChange={(event) =>
                          graphStore.applyGraphFilter({ ...filter, propertyKey: event.target.value })
                        }
                        placeholder="status"
                        disabled={!snapshot}
                      />
                    </label>
                    <label>
                      Property value
                      <input
                        value={filter.propertyValue}
                        onChange={(event) =>
                          graphStore.applyGraphFilter({ ...filter, propertyValue: event.target.value })
                        }
                        placeholder="planned"
                        disabled={!snapshot}
                      />
                    </label>
                    <button
                      type="button"
                      onClick={() => graphStore.applyGraphFilter(DEFAULT_GRAPH_FILTER)}
                      disabled={!snapshot}
                    >
                      Clear
                    </button>
                    <button type="button" onClick={graphStore.recordQueryExecution} disabled={!snapshot}>
                      Summarize
                    </button>
                    <button type="button" onClick={() => void executeQuery()} disabled={queryLoading || (!snapshot && settings.useFixtureData)}>
                      {queryLoading || queryStatus === "running" ? "Running" : "Execute"}
                    </button>
                    <button type="button" onClick={() => void executeQuery("explain")} disabled={queryLoading || (!snapshot && settings.useFixtureData)}>
                      Explain
                    </button>
                    <button type="button" onClick={() => void executeQuery("profile")} disabled={queryLoading || (!snapshot && settings.useFixtureData)}>
                      Profile
                    </button>
                  </>
                ) : (
                  <>
                    <label className="command-input">
                      Schema filter
                      <input
                        value={schemaFilter}
                        onChange={(event) => graphStore.setSchemaFilter(event.target.value)}
                        placeholder="model, field, or edge direction"
                        disabled={!snapshot}
                      />
                    </label>
                    <button type="button" onClick={graphStore.recordQueryExecution} disabled={!snapshot}>
                      Summarize
                    </button>
                  </>
                )}
              </section>
              {queryMessage && (
                <section className={`query-message ${queryStatus}`} aria-live="polite">
                  {queryMessage}
                </section>
              )}
              <div className={`query-workbench ${insightPanelsVisible ? "with-insights" : ""}`}>
                <GraphCanvas
                  snapshot={graphSnapshot}
                  graphView={graphView}
                  onSelect={handleSelect}
                  onHover={setHovered}
                  visualProjection={effectiveVisualProjection}
                />
                <QueryInsightPanels
                  explainVisible={graphView === "data" && explainVisible}
                  profileVisible={graphView === "data" && profileVisible}
                  snapshot={snapshot}
                  visibleSnapshot={visibleSnapshot}
                  filter={filter}
                  graphView={graphView}
                  lastExecutedQuery={lastExecutedQuery}
                  queryEvidence={queryEvidence}
                />
              </div>
            </>
          ) : (
            <EventStreamPanel auditStatus={securityAuditStatus} events={events} />
          )}
        </div>
        <SelectionPanel selected={selectedItem} projection={effectiveVisualProjection} open={selectionPanelOpen} onOpenChange={setSelectionPanelOpen} />
      </div>
    </main>
  );
}
