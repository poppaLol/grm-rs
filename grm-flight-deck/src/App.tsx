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
    <section className="event-band" aria-label="Execution events">
      {events.map((event) => (
        <span className={`event-pill ${event.kind}`} key={event.id}>
          <strong>{event.kind}</strong>
          {event.label}
        </span>
      ))}
    </section>
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
    events
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
      </header>

      <section className="status-line">
        <span className="status-item">
          <span>{status}</span>
          {statusDetail && <span>{statusDetail}</span>}
        </span>
        <SecurityStatusPanel status={securityStatus} />
        {hovered && <span className="hover-preview">{hovered}</span>}
        {lastError && <span className="error">{lastError}</span>}
      </section>

      <section className="query-bar" aria-label="Graph filter">
        <label>
          Search
          <input
            value={filter.text}
            onChange={(event) => graphStore.applyGraphFilter({ ...filter, text: event.target.value })}
            placeholder="id, label, model, property"
          />
        </label>
        <label>
          Model
          <select
            value={filter.model}
            onChange={(event) => graphStore.applyGraphFilter({ ...filter, model: event.target.value })}
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
          />
        </label>
        <button type="button" onClick={() => graphStore.applyGraphFilter(DEFAULT_GRAPH_FILTER)}>
          Clear
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
          <GraphCanvas
            snapshot={visibleSnapshot}
            onSelect={handleSelect}
            onHover={setHovered}
          />
          <EventStreamPanel events={events} />
        </div>
        <SelectionPanel selected={selectedItem} />
      </div>
    </main>
  );
}
