import { FormEvent, useCallback, useMemo, useState } from "react";

import { fetchSecurityStatus, fetchSnapshot } from "./api";
import {
  createFlightDeckGraphStore,
  DEFAULT_GRAPH_FILTER,
  useFlightDeckGraphStore
} from "./graphStore";
import { GraphCanvas } from "./GraphCanvas";
import { colorForModel } from "./modelColors";
import type {
  FlightDeckSecurityStatus,
  FlightDeckSnapshot,
  GraphFilter,
  GraphView,
  QueryExecutionContext,
  SelectedGraphItem
} from "./types";

const graphStore = createFlightDeckGraphStore(window.localStorage);

function SchemaList({ title, models }: { title: string; models: string[] }) {
  return (
    <section className="schema-list-section">
      <h3>{title}</h3>
      <ul className="model-list">
        {models.map((model) => (
          <li key={model}>
            <span
              className="model-swatch"
              style={{ backgroundColor: colorForModel(model) }}
            />
            <span>{model}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function SelectionPanel({ selected }: { selected: SelectedGraphItem | null }) {
  if (!selected) {
    return (
      <aside className="panel inspector">
        <h2>Selection</h2>
        <p className="muted">Select a node or edge to inspect it.</p>
      </aside>
    );
  }

  return (
    <aside className="panel inspector">
      <h2>{selected.label}</h2>
      <p className="kind">{selected.kind} / {selected.model}</p>
      <pre>{JSON.stringify(selected.props, null, 2)}</pre>
    </aside>
  );
}

function EventStreamPanel({ events }: { events: ReturnType<typeof graphStore.getState>["events"] }) {
  return (
    <section className="audit-panel" aria-label="Execution events">
      <div>
        <h2>Audit</h2>
        <p className="muted">Bounded, redacted local observations for this flight-deck session.</p>
      </div>
      <ol className="audit-list">
        {events.map((event) => (
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

function QueryInsightPanels({
  explainVisible,
  profileVisible,
  snapshot,
  visibleSnapshot,
  filter,
  graphView,
  lastExecutedQuery
}: {
  explainVisible: boolean;
  profileVisible: boolean;
  snapshot: FlightDeckSnapshot | null;
  visibleSnapshot: FlightDeckSnapshot | null;
  filter: GraphFilter;
  graphView: GraphView;
  lastExecutedQuery: QueryExecutionContext | null;
}) {
  if (!explainVisible && !profileVisible) {
    return null;
  }

  return (
    <aside className="query-insights" aria-label="Query explain and profile">
      {explainVisible && (
        <section className="insight-panel">
          <h2>Explain</h2>
          <p className="muted">Local view summary, not a service planner result.</p>
          <dl>
            <dt>Operation</dt>
            <dd>{graphView === "schema" ? "schema projection" : "bounded snapshot filter"}</dd>
            <dt>Workspace</dt>
            <dd>{lastExecutedQuery?.workspace ?? snapshot?.workspace ?? "not loaded"}</dd>
            <dt>Source</dt>
            <dd>{snapshot?.source === "service" ? "service snapshot, client-side summary" : "fixture/client-side summary"}</dd>
            <dt>Model scope</dt>
            <dd>{graphView === "schema" ? "schema catalogue" : filter.model || "all models"}</dd>
            <dt>Predicates</dt>
            <dd>{graphView === "schema" ? "not applied to schema view" : predicateSummary(filter)}</dd>
            <dt>Result shape</dt>
            <dd>{resultShapeSummary(snapshot, visibleSnapshot, graphView, lastExecutedQuery)}</dd>
            <dt>Capability</dt>
            <dd>{graphView === "schema" ? "local schema projection" : "local summary only"}</dd>
          </dl>
        </section>
      )}
      {profileVisible && (
        <section className="insight-panel">
          <h2>Profile</h2>
          <p className="muted">Local row counts from the current visible snapshot.</p>
          <dl>
            <dt>Source rows</dt>
            <dd>{sourceRowsSummary(snapshot, graphView, lastExecutedQuery)}</dd>
            <dt>Visible rows</dt>
            <dd>{resultShapeSummary(snapshot, visibleSnapshot, graphView, lastExecutedQuery)}</dd>
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
  const [hovered, setHovered] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

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
    lastExecutedQuery
  } = storeState;

  const modelOptions = useMemo(() => {
    if (!snapshot) {
      return [];
    }
    return [...snapshot.nodeModels, ...snapshot.edgeModels].sort();
  }, [snapshot]);

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
      const loaded = await fetchSnapshot(settings, controller.signal);
      graphStore.loadSnapshot(loaded);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSecurityStatus(null);
      graphStore.markConnectionFailed(message);
    } finally {
      setLoading(false);
    }
  };

  const handleSelect = useCallback((item: SelectedGraphItem | null) => {
    graphStore.selectGraphItem(item ? { kind: item.kind, id: item.id } : null);
  }, []);

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
          <form className="connection-form" onSubmit={load}>
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
            Service base URL
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
              max="1000"
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
          <button type="button" onClick={graphStore.saveCurrentProfile}>
            Save profile
          </button>
          <button type="button" onClick={graphStore.createProfile}>
            New profile
          </button>
          <button type="submit" disabled={loading || settings.workspace.trim() === ""}>
            {loading ? "Loading" : "Connect"}
          </button>
          </form>
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

      <section className="workspace-nav" aria-label="Workspace view">
        <button
          type="button"
          className={activeWorkspacePanel === "query" ? "active" : ""}
          onClick={() => graphStore.selectWorkspacePanel("query")}
        >
          Query
        </button>
        <button
          type="button"
          className={activeWorkspacePanel === "audit" ? "active" : ""}
          onClick={() => graphStore.selectWorkspacePanel("audit")}
        >
          Audit
        </button>
      </section>

      <div className="flight-deck">
        <aside className="panel schema-panel">
          <h2>Schema</h2>
          <SchemaList title="Node models" models={visibleSnapshot?.nodeModels ?? []} />
          <SchemaList title="Edge models" models={visibleSnapshot?.edgeModels ?? []} />
          {visibleSnapshot?.partialReason && (
            <p className="warning">{visibleSnapshot.partialReason}</p>
          )}
          {visibleSnapshot && visibleSnapshot.omittedEdges > 0 && (
            <p className="warning">
              {visibleSnapshot.omittedEdges} edges omitted outside the bounded node result.
            </p>
          )}
        </aside>

        <div className="graph-column">
          {activeWorkspacePanel === "query" ? (
            <>
              <section className="query-bar" aria-label="Graph filter">
                <label>
                  Search
                  <input
                    value={filter.text}
                    onChange={(event) => graphStore.applyGraphFilter({ ...filter, text: event.target.value })}
                    placeholder="id, label, model, property"
                    disabled={!snapshot || graphView === "schema"}
                  />
                </label>
                <label>
                  Model
                  <select
                    value={filter.model}
                    onChange={(event) => graphStore.applyGraphFilter({ ...filter, model: event.target.value })}
                    disabled={!snapshot || graphView === "schema"}
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
                    disabled={!snapshot || graphView === "schema"}
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
                    disabled={!snapshot || graphView === "schema"}
                  />
                </label>
                <button
                  type="button"
                  onClick={() => graphStore.applyGraphFilter(DEFAULT_GRAPH_FILTER)}
                  disabled={!snapshot || graphView === "schema"}
                >
                  Clear
                </button>
                <button type="button" onClick={graphStore.recordQueryExecution} disabled={!snapshot}>
                  Execute
                </button>
                <label className="check-label insight-toggle">
                  <input
                    type="checkbox"
                    checked={explainVisible}
                    onChange={(event) => graphStore.setExplainVisible(event.target.checked)}
                  />
                  Explain
                </label>
                <label className="check-label insight-toggle">
                  <input
                    type="checkbox"
                    checked={profileVisible}
                    onChange={(event) => graphStore.setProfileVisible(event.target.checked)}
                  />
                  Profile
                </label>
              </section>
              <div className={`query-workbench ${explainVisible || profileVisible ? "with-insights" : ""}`}>
                <GraphCanvas
                  snapshot={visibleSnapshot}
                  graphView={graphView}
                  onGraphViewChange={graphStore.setGraphView}
                  onSelect={handleSelect}
                  onHover={setHovered}
                />
                <QueryInsightPanels
                  explainVisible={explainVisible}
                  profileVisible={profileVisible}
                  snapshot={snapshot}
                  visibleSnapshot={visibleSnapshot}
                  filter={filter}
                  graphView={graphView}
                  lastExecutedQuery={lastExecutedQuery}
                />
              </div>
            </>
          ) : (
            <EventStreamPanel events={events} />
          )}
        </div>
        <SelectionPanel selected={selectedItem} />
      </div>
    </main>
  );
}
