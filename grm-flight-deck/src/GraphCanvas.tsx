import { useEffect, useRef, useState } from "react";
import cytoscape, { Core, ElementDefinition } from "cytoscape";

import { colorForModel } from "./modelColors";
import type { FlightDeckSnapshot, GraphView, JsonValue, SelectedGraphItem } from "./types";

interface GraphCanvasProps {
  snapshot: FlightDeckSnapshot | null;
  graphView: GraphView;
  onGraphViewChange: (view: GraphView) => void;
  onSelect: (item: SelectedGraphItem | null) => void;
  onHover: (label: string | null) => void;
}

type LayoutMode = "force" | "groups" | "hierarchy" | "circle" | "grid";
const MIN_RENDERING_MS = 380;
const RENDER_START_DELAY_MS = 35;

export function GraphCanvas({
  snapshot,
  graphView,
  onGraphViewChange,
  onSelect,
  onHover
}: GraphCanvasProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const graphRef = useRef<Core | null>(null);
  const [layoutMode, setLayoutMode] = useState<LayoutMode>("force");
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
              label: "data(displayLabel)",
              shape: "ellipse",
              "background-color": "data(color)",
              "border-color": "#d5f8ff",
              "border-opacity": 0.32,
              "border-width": 1,
              color: "#dce8ea",
              "font-size": "8px",
              "font-weight": 600,
              "min-zoomed-font-size": 5,
              "text-background-color": "#0a0d0e",
              "text-background-opacity": 0.78,
              "text-background-padding": "2px",
              "text-halign": "center",
              "text-margin-y": 8,
              "text-max-width": "96px",
              "text-valign": "bottom",
              "text-wrap": "wrap",
              "overlay-color": "#62c6f2",
              "overlay-opacity": 0,
              "overlay-padding": 4,
              "active-bg-color": "#62c6f2",
              "active-bg-opacity": 0.14,
              "active-bg-size": 24,
              width: 16,
              height: 16
            }
          },
          {
            selector: "node.schema-model",
            style: {
              shape: "round-rectangle",
              width: "data(width)",
              height: "data(height)",
              "background-opacity": 0.9,
              "border-color": "#d5f8ff",
              "border-opacity": 0.55,
              "border-width": 1.5,
              color: "#eef7f8",
              "font-family": "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
              "font-size": "7px",
              "font-weight": 500,
              "text-background-opacity": 0,
              "text-halign": "center",
              "text-margin-y": 0,
              "text-max-width": "data(textWidth)",
              "text-valign": "center",
              "text-wrap": "wrap",
              "text-justification": "left"
            }
          },
          {
            selector: "edge",
            style: {
              label: "data(displayLabel)",
              width: 1,
              "line-color": "#536675",
              "target-arrow-color": "#536675",
              "target-arrow-shape": "triangle",
              "arrow-scale": 0.62,
              "curve-style": "bezier",
              color: "#98aaa9",
              "font-size": "7px",
              "min-zoomed-font-size": 5,
              "text-background-color": "#0a0d0e",
              "text-background-opacity": 0.78,
              "text-background-padding": "2px",
              "text-rotation": "autorotate",
              opacity: 0.56,
              "overlay-opacity": 0,
              "active-bg-opacity": 0
            }
          },
          {
            selector: "edge.self-loop",
            style: {
              width: 2.8,
              "line-color": "#f0c46f",
              "target-arrow-color": "#f0c46f",
              "arrow-scale": 0.95,
              "curve-style": "bezier",
              "control-point-step-size": 112,
              "loop-direction": "-28deg",
              "loop-sweep": "-315deg",
              opacity: 0.94,
              "line-style": "dashed",
              "z-index": 18,
              "text-background-color": "#10171a",
              "text-background-opacity": 0.95,
              "text-background-padding": "3px",
              "text-margin-y": -16,
              color: "#fff2c8",
              "font-size": "8px",
              "font-weight": 800
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
          id: data.id,
          label: data.label,
          model: data.model,
          props: data.props ?? {}
        });
      });

      graph.on("tap", "edge", (event) => {
        const data = event.target.data();
        onSelect({
          kind: "edge",
          id: String(data.sourceId ?? data.id),
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
              onGraphViewChange(event.target.value as GraphView);
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

function graphElements(snapshot: FlightDeckSnapshot, graphView: GraphView): ElementDefinition[] {
  if (graphView === "schema") {
    const schemaNodeModels = schemaNodes(snapshot);
    const elements: ElementDefinition[] = [];
    for (const model of schemaNodeModels) {
      elements.push({
        data: {
          id: `schema-node:${model.name}`,
          label: model.name,
          displayLabel: schemaModelLabel(model),
          model: model.name,
          color: colorForModel(model.name),
          width: schemaModelWidth(model),
          height: schemaModelHeight(model),
          textWidth: Math.max(120, schemaModelWidth(model) - 24),
          props: {
            kind: "node_model",
            idField: model.idField,
            fields: model.fields.map((field) => ({
              name: field.name,
              valueType: field.valueType,
              required: field.required
            }))
          } satisfies Record<string, JsonValue>
        },
        classes: "schema-model"
      });
    }

    for (const [index, edge] of schemaEdges(snapshot).entries()) {
      const source = `schema-node:${edge.fromModel}`;
      const target = `schema-node:${edge.toModel}`;
      const isSelfLoop = source === target;
      elements.push({
        data: {
          id: `schema-edge:${edge.model}:${index}`,
          source,
          target,
          label: edge.model,
          displayLabel: edge.model,
          model: edge.model,
          props: {
            kind: "edge_model",
            fromModel: edge.fromModel,
            toModel: edge.toModel,
            selfLoop: isSelfLoop
          } satisfies Record<string, JsonValue>
        },
        classes: isSelfLoop ? "self-loop" : ""
      });
    }
    return elements;
  }

  const elements: ElementDefinition[] = [];
  for (const node of snapshot.nodes) {
    elements.push({
      data: {
        id: node.id,
        label: node.label,
        displayLabel: compactGraphLabel(node.label),
        model: node.model,
        color: colorForModel(node.model),
        props: node.props
      }
    });
  }

  for (const edge of snapshot.edges) {
    const isSelfLoop = edge.from === edge.to;
    elements.push({
      data: {
        id: `e${edge.id}`,
        sourceId: edge.id,
        source: edge.from,
        target: edge.to,
        label: edge.model,
        displayLabel: isSelfLoop ? edge.model : "",
        model: edge.model,
        props: {
          ...edge.props,
          selfLoop: isSelfLoop
        }
      },
      classes: isSelfLoop ? "self-loop" : ""
    });
  }
  return elements;
}

function schemaNodes(snapshot: FlightDeckSnapshot) {
  if (snapshot.schemaNodeModels && snapshot.schemaNodeModels.length > 0) {
    return snapshot.schemaNodeModels;
  }

  return snapshot.nodeModels.map((name) => {
    const fields = inferNodeModelFields(snapshot, name);
    return {
      name,
      idField: "id",
      fields
    };
  });
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

function compactGraphLabel(label: string): string {
  const withoutModelPrefix = label.replace(/^[A-Za-z][A-Za-z0-9_]*:\s*/, "");
  if (withoutModelPrefix.length <= 34) {
    return withoutModelPrefix;
  }
  return `${withoutModelPrefix.slice(0, 31)}...`;
}

function schemaModelLabel(model: ReturnType<typeof schemaNodes>[number]): string {
  const fieldLines = model.fields.length > 0
    ? model.fields.slice(0, 7).map((field) => {
        const optional = field.required ? "" : "?";
        return `${field.name}${optional}: ${field.valueType}`;
      })
    : [`${model.idField}: id`];
  const omitted = model.fields.length > fieldLines.length
    ? [`+${model.fields.length - fieldLines.length} more`]
    : [];

  return [model.name, "----------", ...fieldLines, ...omitted].join("\n");
}

function schemaModelWidth(model: ReturnType<typeof schemaNodes>[number]): number {
  const longestLine = schemaModelLabel(model)
    .split("\n")
    .reduce((longest, line) => Math.max(longest, line.length), 0);
  return clamp(longestLine * 5.6 + 24, 120, 224);
}

function schemaModelHeight(model: ReturnType<typeof schemaNodes>[number]): number {
  const lineCount = schemaModelLabel(model).split("\n").length;
  return clamp(lineCount * 10.4 + 18, 56, 136);
}

function inferNodeModelFields(snapshot: FlightDeckSnapshot, modelName: string) {
  const fields = new Map<string, { name: string; valueType: string; required: boolean }>();
  for (const node of snapshot.nodes) {
    if (node.model !== modelName) {
      continue;
    }
    for (const [name, value] of Object.entries(node.props)) {
      if (!fields.has(name)) {
        fields.set(name, {
          name,
          valueType: jsonValueType(value),
          required: false
        });
      }
    }
  }
  return [...fields.values()].sort((left, right) => left.name.localeCompare(right.name));
}

function jsonValueType(value: JsonValue): string {
  if (value === null) {
    return "unknown";
  }
  if (Array.isArray(value)) {
    return "array";
  }
  if (typeof value === "object") {
    return "object";
  }
  return typeof value;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
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
        padding: 96,
        name: "breadthfirst",
        directed: true,
        circle: false,
        avoidOverlap: true,
        nodeDimensionsIncludeLabels: false,
        spacingFactor: 2.15,
        grid: false,
        maximal: false,
        transform: (_node, position) => ({
          x: position.x * 0.48,
          y: position.y * 3.4
        })
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
