import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";

import { fetchSnapshot, filterSnapshot } from "./api";
import { GraphCanvas } from "./GraphCanvas";
import { colorForModel } from "./modelColors";
import type {
  ConnectionSettings,
  FlightDeckEvent,
  FlightDeckSnapshot,
  GraphFilter,
  SelectedGraphItem
} from "./types";

const STORAGE_KEY = "grm-flight-deck.connection.v2";
const DEFAULT_SETTINGS: ConnectionSettings = {
  serviceBaseUrl: "",
  mode: "local-anonymous-dev",
  workspace: "flight-deck-demo",
  limit: 50,
  useFixtureData: true
};

const DEFAULT_FILTER: GraphFilter = {
  text: "",
  model: "",
  propertyKey: "",
  propertyValue: ""
};

function readSettings(): ConnectionSettings {
  const raw = window.localStorage.getItem(STORAGE_KEY);
  if (!raw) {
    return DEFAULT_SETTINGS;
  }

  try {
    return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

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

function EventStreamPanel({ events }: { events: FlightDeckEvent[] }) {
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

export function App() {
  const [settings, setSettings] = useState(readSettings);
  const [filter, setFilter] = useState(DEFAULT_FILTER);
  const [snapshot, setSnapshot] = useState<FlightDeckSnapshot | null>(null);
  const [selected, setSelected] = useState<SelectedGraphItem | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const [status, setStatus] = useState("Ready for a local service connection.");
  const [statusDetail, setStatusDetail] = useState("");
  const [lastError, setLastError] = useState("");
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  }, [settings]);

  const visibleSnapshot = useMemo(() => {
    return snapshot ? filterSnapshot(snapshot, filter) : null;
  }, [filter, snapshot]);

  const modelOptions = useMemo(() => {
    if (!snapshot) {
      return [];
    }
    return [...snapshot.nodeModels, ...snapshot.edgeModels].sort();
  }, [snapshot]);

  const events = useMemo<FlightDeckEvent[]>(() => {
    if (!snapshot) {
      return [
        { id: "idle", kind: "read", label: "event hook idle", status: "fixture" }
      ];
    }

    return [
      {
        id: "snapshot-read",
        kind: "read",
        label: `${snapshot.nodes.length} nodes observed`,
        status: snapshot.source === "fixture" ? "fixture" : "observed"
      },
      {
        id: "filter-boundary",
        kind: "edge-traversed",
        label: `${visibleSnapshot?.edges.length ?? 0} edges visible`,
        status: snapshot.source === "fixture" ? "fixture" : "observed"
      }
    ];
  }, [snapshot, visibleSnapshot]);

  const load = async (event?: FormEvent) => {
    event?.preventDefault();
    setLoading(true);
    setSelected(null);
    setLastError("");
    setStatus(settings.useFixtureData ? "Loading fixture snapshot..." : "Connecting to service...");
    setStatusDetail("");

    const controller = new AbortController();
    try {
      const loaded = await fetchSnapshot(settings, controller.signal);
      setSnapshot(loaded);
      setStatus(`${loaded.source} snapshot:`);
      setStatusDetail(
        `${loaded.nodes.length} nodes / ${loaded.edges.length} edges / limit ${loaded.modelLimit}`
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSnapshot(null);
      setLastError(message);
      setStatus("Connection failed.");
      setStatusDetail("");
    } finally {
      setLoading(false);
    }
  };

  const handleSelect = useCallback((item: SelectedGraphItem | null) => {
    setSelected(item);
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
            Service base URL
            <input
              placeholder="blank uses /api proxy, or http://127.0.0.1:3001"
              value={settings.serviceBaseUrl}
              onChange={(event) =>
                setSettings({ ...settings, serviceBaseUrl: event.target.value })
              }
              disabled={settings.useFixtureData}
            />
          </label>
          <label>
            Workspace
            <input
              value={settings.workspace}
              onChange={(event) =>
                setSettings({ ...settings, workspace: event.target.value })
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
                setSettings({ ...settings, limit: Number(event.target.value) })
              }
            />
          </label>
          <label className="check-label">
            <input
              type="checkbox"
              checked={settings.useFixtureData}
              onChange={(event) =>
                setSettings({ ...settings, useFixtureData: event.target.checked })
              }
            />
            Fixture
          </label>
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
        <span className="status-item status-mode">Mode: {settings.mode}</span>
        {hovered && <span className="hover-preview">{hovered}</span>}
        {lastError && <span className="error">{lastError}</span>}
      </section>

      <section className="query-bar" aria-label="Graph filter">
        <label>
          Search
          <input
            value={filter.text}
            onChange={(event) => setFilter({ ...filter, text: event.target.value })}
            placeholder="id, label, model, property"
          />
        </label>
        <label>
          Model
          <select
            value={filter.model}
            onChange={(event) => setFilter({ ...filter, model: event.target.value })}
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
              setFilter({ ...filter, propertyKey: event.target.value })
            }
            placeholder="status"
          />
        </label>
        <label>
          Property value
          <input
            value={filter.propertyValue}
            onChange={(event) =>
              setFilter({ ...filter, propertyValue: event.target.value })
            }
            placeholder="planned"
          />
        </label>
        <button type="button" onClick={() => setFilter(DEFAULT_FILTER)}>
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
        <SelectionPanel selected={selected} />
      </div>
    </main>
  );
}
