import { useEffect, useRef, useState } from "react";
import cytoscape, { Core } from "cytoscape";

import { colorForModel } from "./modelColors";
import type { FlightDeckSnapshot, JsonValue, SelectedGraphItem } from "./types";

interface GraphCanvasProps {
  snapshot: FlightDeckSnapshot | null;
  onSelect: (item: SelectedGraphItem | null) => void;
  onHover: (label: string | null) => void;
}

type LayoutMode = "force" | "groups" | "hierarchy" | "circle" | "grid";
type GraphView = "data" | "schema";
const MIN_RENDERING_MS = 380;
const RENDER_START_DELAY_MS = 35;

export function GraphCanvas({ snapshot, onSelect, onHover }: GraphCanvasProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const graphRef = useRef<Core | null>(null);
  const [layoutMode, setLayoutMode] = useState<LayoutMode>("force");
  const [graphView, setGraphView] = useState<GraphView>("data");
  const [rendering, setRendering] = useState(false);

  useEffect(() => {
    if (!containerRef.current || !snapshot) {
      return;
    }

    let cancelled = false;
    let startTimer = 0;
    let clearTimer = 0;
    setRendering(true);
    const renderStartedAt = window.performance.now();

    startTimer = window.setTimeout(() => {
      if (cancelled || !containerRef.current) {
        return;
      }

      graphRef.current?.destroy();
      const graph = cytoscape({
        container: containerRef.current,
        elements: graphElements(snapshot, graphView),
        layout: { name: "preset" },
        style: [
          {
            selector: "node",
            style: {
              label: "",
              shape: "ellipse",
              "background-color": "data(color)",
              "border-color": "#d5f8ff",
              "border-opacity": 0.32,
              "border-width": 1,
              "overlay-color": "#62c6f2",
              "overlay-opacity": 0,
              "overlay-padding": 4,
              "active-bg-color": "#62c6f2",
              "active-bg-opacity": 0.14,
              "active-bg-size": 24,
              width: 17,
              height: 17
            }
          },
          {
            selector: "edge",
            style: {
              width: 1,
              "line-color": "#536675",
              "target-arrow-color": "#536675",
              "target-arrow-shape": "triangle",
              "arrow-scale": 0.62,
              "curve-style": "bezier",
              opacity: 0.56,
              "overlay-opacity": 0,
              "active-bg-opacity": 0
            }
          },
          {
            selector: ":selected",
            style: {
              "background-color": "#62c6f2",
              "line-color": "#62c6f2",
              "target-arrow-color": "#62c6f2",
              "border-color": "#fff2c8",
              "border-width": 2
            }
          }
        ]
      });

      graphRef.current = graph;

      graph.on("tap", "node", (event) => {
        const data = event.target.data();
        onSelect({
          kind: "node",
          label: data.label,
          model: data.model,
          props: data.props ?? {}
        });
      });

      graph.on("tap", "edge", (event) => {
        const data = event.target.data();
        onSelect({
          kind: "edge",
          label: data.label,
          model: data.model,
          props: data.props ?? {}
        });
      });

      graph.on("tap", (event) => {
        if (event.target === graphRef.current) {
          onSelect(null);
        }
      });

      graph.on("mouseover", "node, edge", (event) => {
        const data = event.target.data();
        onHover(`${data.model}: ${data.label}`);
      });

      graph.on("mouseout", "node, edge", () => {
        onHover(null);
      });

      graph.one("layoutstop", () => {
        const elapsed = window.performance.now() - renderStartedAt;
        const remaining = Math.max(0, MIN_RENDERING_MS - elapsed);
        clearTimer = window.setTimeout(() => {
          if (!cancelled) {
            setRendering(false);
          }
        }, remaining);
      });
      graph.layout(layoutOptions(layoutMode, snapshot)).run();
    }, RENDER_START_DELAY_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(startTimer);
      window.clearTimeout(clearTimer);
      graphRef.current?.destroy();
      graphRef.current = null;
    };
  }, [graphView, layoutMode, snapshot, onHover, onSelect]);

  const fitToView = () => {
    graphRef.current?.fit(undefined, 72);
  };

  return (
    <section className="graph-shell" aria-label="Graph visualisation">
      <div className="graph-toolbar">
        <label className="layout-control">
          View
          <select
            value={graphView}
            onChange={(event) => {
              setRendering(true);
              setGraphView(event.target.value as GraphView);
            }}
            disabled={!snapshot}
          >
            <option value="data">Data</option>
            <option value="schema">Schema</option>
          </select>
        </label>
        <label className="layout-control">
          Layout
          <select
            value={layoutMode}
            onChange={(event) => {
              setRendering(true);
              setLayoutMode(event.target.value as LayoutMode);
            }}
            disabled={!snapshot}
          >
            <option value="force">Force</option>
            <option value="groups">Groups</option>
            <option value="hierarchy">Hierarchy</option>
            <option value="circle">Circle</option>
            <option value="grid">Grid</option>
          </select>
        </label>
        <button type="button" onClick={fitToView} disabled={!snapshot}>
          Fit
        </button>
      </div>
      <div ref={containerRef} className="graph-canvas" />
      {!snapshot && <div className="empty-state">Load a bounded workspace snapshot.</div>}
      {snapshot && graphView === "data" && snapshot.nodes.length === 0 && (
        <div className="empty-state">No graph items match the current filter.</div>
      )}
      {snapshot && graphView === "schema" && snapshot.nodeModels.length === 0 && (
        <div className="empty-state">No schema models are available.</div>
      )}
      {rendering && (
        <div className="rendering-overlay" role="status" aria-live="polite">
          <span className="spinner" />
          <span>Rendering graph</span>
        </div>
      )}
    </section>
  );
}

function graphElements(snapshot: FlightDeckSnapshot, graphView: GraphView) {
  if (graphView === "schema") {
    return [
      ...snapshot.nodeModels.map((model) => ({
        data: {
          id: `schema-node:${model}`,
          label: model,
          model,
          color: colorForModel(model),
          props: { kind: "node_model" } satisfies Record<string, JsonValue>
        }
      })),
      ...schemaEdges(snapshot).map((edge, index) => ({
        data: {
          id: `schema-edge:${edge.model}:${index}`,
          source: `schema-node:${edge.fromModel}`,
          target: `schema-node:${edge.toModel}`,
          label: edge.model,
          model: edge.model,
          props: {
            kind: "edge_model",
            fromModel: edge.fromModel,
            toModel: edge.toModel
          } satisfies Record<string, JsonValue>
        }
      }))
    ];
  }

  return [
    ...snapshot.nodes.map((node) => ({
      data: {
        id: node.id,
        label: node.label,
        model: node.model,
        color: colorForModel(node.model),
        props: node.props
      }
    })),
    ...snapshot.edges.map((edge) => ({
      data: {
        id: `e${edge.id}`,
        source: edge.from,
        target: edge.to,
        label: edge.model,
        model: edge.model,
        props: edge.props
      }
    }))
  ];
}

function schemaEdges(snapshot: FlightDeckSnapshot) {
  if (snapshot.schemaEdges && snapshot.schemaEdges.length > 0) {
    return snapshot.schemaEdges;
  }

  const nodesById = new Map(snapshot.nodes.map((node) => [node.id, node]));
  const inferredEdges = new Map<string, { model: string; fromModel: string; toModel: string }>();

  for (const edge of snapshot.edges) {
    const from = nodesById.get(edge.from);
    const to = nodesById.get(edge.to);
    if (!from || !to) {
      continue;
    }

    const key = `${edge.model}:${from.model}:${to.model}`;
    inferredEdges.set(key, {
      model: edge.model,
      fromModel: from.model,
      toModel: to.model
    });
  }

  return [...inferredEdges.values()];
}

function layoutOptions(layoutMode: LayoutMode, snapshot: FlightDeckSnapshot): cytoscape.LayoutOptions {
  const base = {
    animate: false,
    fit: true,
    padding: 72
  };

  switch (layoutMode) {
    case "groups": {
      const orderedModels = [...snapshot.nodeModels].sort();
      return {
        ...base,
        name: "concentric",
        minNodeSpacing: 48,
        concentric: (node) => {
          const model = String(node.data("model"));
          const index = orderedModels.indexOf(model);
          return index === -1 ? 0 : orderedModels.length - index;
        },
        levelWidth: () => 1
      };
    }
    case "hierarchy":
      return {
        ...base,
        name: "breadthfirst",
        directed: true,
        spacingFactor: 1.35,
        circle: false
      };
    case "circle":
      return {
        ...base,
        name: "circle",
        radius: Math.max(120, snapshot.nodes.length * 7),
        spacingFactor: 1.15
      };
    case "grid":
      return {
        ...base,
        name: "grid",
        avoidOverlap: true,
        avoidOverlapPadding: 24,
        condense: false
      };
    case "force":
    default:
      return {
        ...base,
        name: "cose",
        nodeRepulsion: 36000,
        nodeOverlap: 22,
        idealEdgeLength: 210,
        edgeElasticity: 90,
        nestingFactor: 1.2,
        gravity: 0.05,
        numIter: 1600,
        componentSpacing: 185
      };
  }
}
