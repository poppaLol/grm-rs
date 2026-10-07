import { useEffect, useMemo, useRef, useState } from "react";
import cytoscape, { Core, ElementDefinition } from "cytoscape";
import { Maximize, PanelRightOpen, RotateCcw } from "lucide-react";

import { colorForModel, colorForToken } from "./modelColors";
import { buildVisualContainerMap, type VisualContainerMap } from "./containerMap";
import type { VisualProjectionContainerOverlay } from "./projectionOverlay";
import type { FlightDeckSnapshot, FlightDeckVisualProjection, GraphView, JsonValue, SelectedGraphItem } from "./types";

interface GraphCanvasProps {
  snapshot: FlightDeckSnapshot | null;
  graphView: GraphView;
  onSelect: (item: SelectedGraphItem | null) => void;
  onHover: (label: string | null) => void;
  visualProjection: FlightDeckVisualProjection | null;
  visualLayoutMode?: string;
  visualLayoutStyle?: string;
  visualContainers?: Record<string, VisualProjectionContainerOverlay>;
  onLayoutStyleChange?: (style: string) => void;
  onOpenDetails?: () => void;
}

type LayoutMode = "force" | "groups" | "hierarchy" | "circle" | "grid";
const MIN_RENDERING_MS = 380;
const RENDER_START_DELAY_MS = 35;

export function GraphCanvas({
  snapshot,
  graphView,
  onSelect,
  onHover,
  visualProjection,
  visualLayoutMode,
  visualLayoutStyle,
  visualContainers,
  onLayoutStyleChange,
  onOpenDetails
}: GraphCanvasProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const reticuleRef = useRef<HTMLDivElement | null>(null);
  const graphRef = useRef<Core | null>(null);
  const manualPositionsRef = useRef<{ snapshot: FlightDeckSnapshot; graphView: GraphView; positions: Record<string, cytoscape.Position> } | null>(null);
  const [manuallyArranged, setManuallyArranged] = useState(false);
  const [layoutRevision, setLayoutRevision] = useState(0);
  const [layoutModes, setLayoutModes] = useState<Record<GraphView, LayoutMode>>({ data: "force", schema: "grid" });
  const layoutMode = layoutModes[graphView];
  const setLayoutMode = (mode: LayoutMode) => setLayoutModes((modes) => ({ ...modes, [graphView]: mode }));
  const containerMap = useMemo(() => snapshot && graphView === "data" && visualLayoutMode === "container-map"
    ? buildVisualContainerMap(snapshot, visualContainers ?? {})
    : null, [snapshot, graphView, visualLayoutMode, visualContainers]);

  useEffect(() => {
    if (graphView !== "data") return;
    const preferred = layoutModeFromVisualIntent(visualLayoutMode, visualLayoutStyle) ?? "force";
    if (preferred) {
      manualPositionsRef.current = null;
      setManuallyArranged(false);
      setRendering(true);
      setLayoutMode(preferred);
      setLayoutRevision((revision) => revision + 1);
    }
  }, [graphView, visualLayoutMode, visualLayoutStyle]);
  const [rendering, setRendering] = useState(false);

  useEffect(() => {
    if (!containerRef.current || !snapshot) {
      return;
    }

    let cancelled = false;
    let startTimer = 0;
    let clearTimer = 0;
    let resizeObserver: ResizeObserver | undefined;
    setRendering(true);
    const renderStartedAt = window.performance.now();

    startTimer = window.setTimeout(() => {
      if (cancelled || !containerRef.current) {
        return;
      }

      graphRef.current?.destroy();
      const graph = cytoscape({
        container: containerRef.current,
        elements: graphElements(snapshot, graphView, visualProjection, containerMap),
        layout: { name: "preset" },
        style: [
          {
            selector: "node",
            style: {
              label: "data(displayLabel)",
              shape: "data(shape)" as cytoscape.Css.NodeShape,
              "background-color": "data(color)",
              "border-color": "data(roleBorderColor)",
              "border-opacity": "data(roleBorderOpacity)" as unknown as number,
              "border-width": "data(roleBorderWidth)",
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
              width: "data(nodeWidth)",
              height: "data(nodeHeight)"
            }
          },
          {
            selector: "node.visual-container",
            style: {
              shape: "round-rectangle",
              "background-opacity": 0.08,
              "border-width": 1.5,
              "border-color": "data(color)",
              "border-opacity": 0.65,
              padding: "30px",
              "text-valign": "top",
              "text-margin-y": -10,
              "text-max-width": "240px",
              "font-size": "10px",
              "text-background-opacity": 0,
              "compound-sizing-wrt-labels": "include"
            }
          },
          {
            selector: "node.visual-container.render-card",
            style: { "background-opacity": 0.18, "border-width": 2 }
          },
          {
            selector: "node.visual-container.render-lane",
            style: { shape: "rectangle", "border-style": "dashed" }
          },
          {
            selector: "node.visual-container.render-section",
            style: { shape: "rectangle", "background-opacity": 0.03, "border-style": "dotted" }
          },
          {
            selector: "node.visual-container.collapsed",
            style: {
              width: 210, height: 64,
              "background-opacity": 0.2,
              "text-valign": "center",
              "text-margin-y": 0,
              "text-max-width": "190px"
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
              "font-size": "11px",
              "min-zoomed-font-size": 0,
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
              width: "data(edgeWidth)" as unknown as number,
              "line-color": "data(edgeColor)",
              "target-arrow-color": "data(edgeColor)",
              "target-arrow-shape": "triangle",
              "arrow-scale": "data(arrowScale)" as unknown as number,
              "curve-style": "bezier",
              color: "#98aaa9",
              "font-size": "7px",
              "min-zoomed-font-size": 5,
              "text-background-color": "#0a0d0e",
              "text-background-opacity": 0.78,
              "text-background-padding": "2px",
              "text-rotation": "autorotate",
              opacity: "data(edgeOpacity)" as unknown as number,
              "line-style": "data(edgeLineStyle)" as cytoscape.Css.LineStyle,
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
            selector: "node:selected",
            style: {
              "overlay-opacity": 0,
              "underlay-opacity": 0,
              "z-index": 30
            }
          },
          {
            selector: "edge:selected",
            style: {
              opacity: 0.96,
              width: 2.2,
              "line-color": "#ffd166",
              "source-arrow-color": "#ffd166",
              "target-arrow-color": "#ffd166",
              "overlay-opacity": 0,
              "z-index": 28
            }
          }
        ]
      });

      graphRef.current = graph;

      graph.on("dragfree", "node", () => {
        const positions: Record<string, cytoscape.Position> = {};
        graph.nodes().forEach((node) => { positions[node.id()] = { ...node.position() }; });
        manualPositionsRef.current = { snapshot, graphView, positions };
        setManuallyArranged(true);
      });

      const updateReticule = () => updateNodeReticule(graph, reticuleRef.current);

      graph.on("select unselect position render pan zoom", "node", updateReticule);
      graph.on("pan zoom render resize", updateReticule);

      graph.on("tap", "node", (event) => {
        const data = event.target.data();
        onSelect({
          kind: "node",
          id: data.anchorId ?? data.id,
          label: data.label,
          model: data.model,
          props: data.props ?? {}
        });
        window.requestAnimationFrame(updateReticule);
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
          hideNodeReticule(reticuleRef.current);
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
        updateReticule();
        const elapsed = window.performance.now() - renderStartedAt;
        const remaining = Math.max(0, MIN_RENDERING_MS - elapsed);
        clearTimer = window.setTimeout(() => {
          if (!cancelled) {
            setRendering(false);
          }
        }, remaining);
      });
      const runLayout = () => graph.layout(containerMap?.regions.length
        ? containerLayoutOptions(graph, containerMap)
        : layoutOptions(layoutMode, snapshot, visualProjection, graphView, graph)).run();
      const manual = manualPositionsRef.current;
      if (manual?.snapshot === snapshot && manual.graphView === graphView) {
        graph.layout({ name: "preset", positions: manual.positions, fit: true, padding: 72 }).run();
      } else {
        manualPositionsRef.current = null;
        setManuallyArranged(false);
        runLayout();
      }
      let canvasWidth = graph.width();
      let canvasHeight = graph.height();
      resizeObserver = new ResizeObserver(() => {
        if (!cancelled) {
          graph.resize();
          if (graph.width() !== canvasWidth || graph.height() !== canvasHeight) {
            canvasWidth = graph.width();
            canvasHeight = graph.height();
            if (!manualPositionsRef.current) runLayout();
          }
          if (!manualPositionsRef.current) graph.fit(undefined, 72);
        }
      });
      resizeObserver.observe(containerRef.current);
    }, RENDER_START_DELAY_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(startTimer);
      window.clearTimeout(clearTimer);
      resizeObserver?.disconnect();
      hideNodeReticule(reticuleRef.current);
      graphRef.current?.destroy();
      graphRef.current = null;
    };
  }, [graphView, layoutMode, layoutRevision, snapshot, onHover, onSelect, visualProjection, visualLayoutMode, visualLayoutStyle, containerMap]);

  const fitToView = () => {
    graphRef.current?.fit(undefined, 72);
  };

  return (
    <section className="graph-shell" aria-label="Graph visualisation">
      <div className="graph-toolbar">
        <label className="layout-control">
          Layout
          <span className="layout-select-row">
          <select
            aria-label="Layout"
            value={containerMap?.regions.length ? "map" : layoutMode}
            onChange={(event) => {
              manualPositionsRef.current = null;
              setManuallyArranged(false);
              setRendering(true);
              setLayoutMode(event.target.value as LayoutMode);
              setLayoutRevision((revision) => revision + 1);
              if (graphView === "data") onLayoutStyleChange?.(event.target.value);
            }}
            disabled={!snapshot || Boolean(containerMap?.regions.length)}
          >
            {Boolean(containerMap?.regions.length) && <option value="map">Container map</option>}
            <option value="force">Force</option>
            <option value="groups">Groups</option>
            <option value="hierarchy">Hierarchy</option>
            <option value="circle">Circle</option>
            <option value="grid">Grid</option>
          </select>
          {manuallyArranged && <span className="layout-modified" title="Nodes moved from the automatic layout" aria-label="Arrangement moved">*</span>}
          </span>
        </label>
        {manuallyArranged && <button type="button" className="icon-button" title="Restore automatic layout" aria-label="Restore automatic layout" onClick={() => {
          manualPositionsRef.current = null;
          setManuallyArranged(false);
          setRendering(true);
          setLayoutRevision((revision) => revision + 1);
        }}><RotateCcw size={18} aria-hidden="true" /></button>}
        <button type="button" className="icon-button" title="Fit graph to view" aria-label="Fit" onClick={fitToView} disabled={!snapshot}>
          <Maximize size={18} aria-hidden="true" />
        </button>
        {onOpenDetails && <button type="button" className="icon-button" title="Open selection details" aria-label="Open selection details" aria-expanded={false} onClick={onOpenDetails}><PanelRightOpen size={18} aria-hidden="true" /></button>}
        {containerMap && (
          <span className="container-map-summary" role="status">
            {containerMap.regions.length} {containerMap.regions.length === 1 ? "region" : "regions"} / {containerMap.regions.filter((region) => region.collapsed).length} collapsed
          </span>
        )}
      </div>
      <div ref={reticuleRef} className="selection-reticule" aria-hidden="true" />
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

function isLayoutMode(value: string | undefined): value is LayoutMode {
  return value === "force" || value === "groups" || value === "hierarchy" || value === "circle" || value === "grid";
}

function layoutModeFromVisualIntent(
  visualLayoutMode: string | undefined,
  visualLayoutStyle: string | undefined
): LayoutMode | null {
  if (isLayoutMode(visualLayoutStyle)) {
    return visualLayoutStyle;
  }
  if (visualLayoutMode === "container-map") {
    return "groups";
  }
  if (visualLayoutMode === "edge-network") {
    return "force";
  }
  return null;
}

function updateNodeReticule(graph: Core, reticule: HTMLDivElement | null) {
  if (!reticule) {
    return;
  }
  const selectedNode = graph.nodes(":selected").first();
  if (!selectedNode || selectedNode.empty()) {
    hideNodeReticule(reticule);
    return;
  }

  const box = selectedNode.renderedBoundingBox({ includeLabels: false, includeOverlays: false });
  const padding = selectedNode.hasClass("schema-model") ? 8 : 9;
  const left = box.x1 - padding;
  const top = box.y1 - padding;
  const width = box.w + padding * 2;
  const height = box.h + padding * 2;

  if (box.x2 < 0 || box.y2 < 0 || box.x1 > graph.width() || box.y1 > graph.height()) {
    hideNodeReticule(reticule);
    return;
  }

  reticule.style.opacity = "1";
  reticule.style.transform = `translate(${left}px, ${top}px)`;
  reticule.style.width = `${width}px`;
  reticule.style.height = `${height}px`;
}

function hideNodeReticule(reticule: HTMLDivElement | null) {
  if (!reticule) {
    return;
  }
  reticule.style.opacity = "0";
}


function graphElements(snapshot: FlightDeckSnapshot, graphView: GraphView, visualProjection: FlightDeckVisualProjection | null, containerMap: VisualContainerMap | null): ElementDefinition[] {
  if (graphView === "schema") {
    const schemaNodeModels = schemaNodes(snapshot);
    const elements: ElementDefinition[] = [];
    for (const model of schemaNodeModels) {
      elements.push({
        data: {
          id: `schema-node:${model.name}`,
          label: schemaNodeProjection(visualProjection, model.name)?.label ?? model.name,
          displayLabel: schemaModelLabel(model, visualProjection),
          model: model.name,
          group: schemaNodeProjection(visualProjection, model.name)?.group ?? model.name,
          color: colorForToken(schemaNodeProjection(visualProjection, model.name)?.colorToken, model.name),
          shape: visualNodeShape(schemaNodeProjection(visualProjection, model.name)?.shape, true),
          roleBorderColor: visualRoleBorder(schemaNodeProjection(visualProjection, model.name)?.visualRole),
          roleBorderOpacity: visualRoleOpacity(schemaNodeProjection(visualProjection, model.name)?.visualRole),
          roleBorderWidth: visualRoleWidth(schemaNodeProjection(visualProjection, model.name)?.visualRole),
          nodeWidth: schemaModelWidth(model),
          nodeHeight: schemaModelHeight(model),
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
          label: schemaEdgeProjection(visualProjection, edge.model)?.label ?? edge.model,
          displayLabel: schemaEdgeProjection(visualProjection, edge.model)?.label ?? edge.model,
          model: edge.model,
          group: schemaEdgeProjection(visualProjection, edge.model)?.group ?? edge.model,
          edgeWidth: visualEdgeWidth(schemaEdgeProjection(visualProjection, edge.model)?.lineWeight, schemaEdgeProjection(visualProjection, edge.model)?.styleToken),
          edgeLineStyle: visualEdgeLineStyle(schemaEdgeProjection(visualProjection, edge.model)?.lineStyle, schemaEdgeProjection(visualProjection, edge.model)?.styleToken),
          edgeColor: visualEdgeColor(schemaEdgeProjection(visualProjection, edge.model)?.styleToken),
          edgeOpacity: visualEdgeOpacity(schemaEdgeProjection(visualProjection, edge.model)?.directionEmphasis),
          arrowScale: visualArrowScale(schemaEdgeProjection(visualProjection, edge.model)?.directionEmphasis),
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
  const regions = new Map(containerMap?.regions.map((region) => [region.id, region]));
  const nodeById = new Map(snapshot.nodes.map((node) => [node.id, node]));
  for (const region of regions.values()) {
    const anchor = nodeById.get(region.anchorId)!;
    elements.push({
      data: {
        id: region.id,
        anchorId: anchor.id,
        label: projectedNodeLabel(anchor, visualProjection),
        displayLabel: `${compactGraphLabel(projectedNodeLabel(anchor, visualProjection))}\n${region.memberIds.length} ${region.memberIds.length === 1 ? "item" : "items"}`,
        model: anchor.model,
        color: colorForToken(region.styleToken ?? schemaNodeProjection(visualProjection, anchor.model)?.colorToken, anchor.model),
        shape: "round-rectangle",
        nodeWidth: 210, nodeHeight: 64,
        roleBorderColor: "#d5f8ff", roleBorderOpacity: 0.65, roleBorderWidth: 1.5,
        props: anchor.props
      },
      classes: `visual-container render-${region.renderAs}${region.collapsed ? " collapsed" : ""}`
    });
  }
  for (const node of snapshot.nodes) {
    const owner = containerMap?.ownerByNode.get(node.id);
    if (owner && regions.get(owner)?.collapsed) {
      continue;
    }
    elements.push({
      data: {
        id: node.id,
        ...(owner ? { parent: owner } : {}),
        label: projectedNodeLabel(node, visualProjection),
        displayLabel: compactGraphLabel(projectedNodeLabel(node, visualProjection)),
        model: node.model,
        group: schemaNodeProjection(visualProjection, node.model)?.group ?? node.model,
        color: colorForToken(schemaNodeProjection(visualProjection, node.model)?.colorToken, node.model),
        shape: visualNodeShape(schemaNodeProjection(visualProjection, node.model)?.shape, false),
        roleBorderColor: visualRoleBorder(schemaNodeProjection(visualProjection, node.model)?.visualRole),
        roleBorderOpacity: visualRoleOpacity(schemaNodeProjection(visualProjection, node.model)?.visualRole),
        roleBorderWidth: visualRoleWidth(schemaNodeProjection(visualProjection, node.model)?.visualRole),
        nodeWidth: visualNodeWidth(schemaNodeProjection(visualProjection, node.model)?.shape, schemaNodeProjection(visualProjection, node.model)?.detailDensity),
        nodeHeight: visualNodeHeight(schemaNodeProjection(visualProjection, node.model)?.shape, schemaNodeProjection(visualProjection, node.model)?.detailDensity),
        props: node.props
      }
    });
  }

  for (const edge of snapshot.edges) {
    if (!nodeById.has(edge.from) || !nodeById.has(edge.to)) {
      continue;
    }
    const fromOwner = containerMap?.ownerByNode.get(edge.from);
    const toOwner = containerMap?.ownerByNode.get(edge.to);
    const source = fromOwner && regions.get(fromOwner)?.collapsed ? fromOwner : edge.from;
    const target = toOwner && regions.get(toOwner)?.collapsed ? toOwner : edge.to;
    if (source === target && (source !== edge.from || target !== edge.to)) {
      continue;
    }
    const isSelfLoop = edge.from === edge.to;
    elements.push({
      data: {
        id: `e${edge.id}`,
        sourceId: edge.id,
        source,
        target,
        label: schemaEdgeProjection(visualProjection, edge.model)?.label ?? edge.model,
        displayLabel: visualEdgeLabel(edge.model, isSelfLoop, visualProjection),
        model: edge.model,
        group: schemaEdgeProjection(visualProjection, edge.model)?.group ?? edge.model,
        edgeWidth: visualEdgeWidth(schemaEdgeProjection(visualProjection, edge.model)?.lineWeight, schemaEdgeProjection(visualProjection, edge.model)?.styleToken),
        edgeLineStyle: visualEdgeLineStyle(schemaEdgeProjection(visualProjection, edge.model)?.lineStyle, schemaEdgeProjection(visualProjection, edge.model)?.styleToken),
        edgeColor: visualEdgeColor(schemaEdgeProjection(visualProjection, edge.model)?.styleToken),
        edgeOpacity: visualEdgeOpacity(schemaEdgeProjection(visualProjection, edge.model)?.directionEmphasis),
        arrowScale: visualArrowScale(schemaEdgeProjection(visualProjection, edge.model)?.directionEmphasis),
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

function containerLayoutOptions(graph: Core, map: VisualContainerMap): cytoscape.LayoutOptions {
  const positions: Record<string, cytoscape.Position> = {};
  const columns = Math.max(1, Math.ceil(Math.sqrt(map.regions.length + graph.nodes().filter((node) => !node.isParent() && !node.parent().length && !node.hasClass("visual-container")).length)));
  let index = 0;
  let x = 0;
  let y = 0;
  let rowHeight = 0;
  const place = (ids: string[], lane = false) => {
    const innerColumns = lane ? ids.length : Math.min(3, Math.ceil(Math.sqrt(ids.length)));
    const width = Math.max(310, innerColumns * 160 + 80);
    const height = Math.max(150, Math.ceil(ids.length / innerColumns) * 100 + 100);
    ids.forEach((id, member) => {
      positions[id] = { x: x + 80 + (member % innerColumns) * 160, y: y + 80 + Math.floor(member / innerColumns) * 100 };
    });
    x += width + 80;
    rowHeight = Math.max(rowHeight, height);
    index += 1;
    if (index % columns === 0) {
      x = 0;
      y += rowHeight + 80;
      rowHeight = 0;
    }
  };
  for (const region of map.regions) {
    place(region.collapsed ? [region.id] : [region.anchorId, ...region.memberIds], region.renderAs === "lane");
  }
  for (const node of graph.nodes().filter((node) => !node.isParent() && !node.parent().length && !node.hasClass("visual-container"))) {
    place([node.id()]);
  }
  return { name: "preset", positions, fit: true, padding: 72, animate: false };
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


function schemaNodeProjection(projection: FlightDeckVisualProjection | null, model: string) {
  return projection?.nodeModels.find((hint) => hint.model === model) ?? null;
}

function schemaEdgeProjection(projection: FlightDeckVisualProjection | null, model: string) {
  return projection?.edgeModels.find((hint) => hint.model === model) ?? null;
}

function projectedNodeLabel(
  node: FlightDeckSnapshot["nodes"][number],
  projection: FlightDeckVisualProjection | null
): string {
  const hint = schemaNodeProjection(projection, node.model);
  const semanticField = hint?.detailFields.find((field) => field !== hint.idField && node.props[field] !== undefined);
  if (semanticField) {
    return `${hint?.label ?? node.model}: ${String(node.props[semanticField])}`;
  }
  return node.label;
}

function orderSchemaFields(
  fields: ReturnType<typeof schemaNodes>[number]["fields"],
  detailFields: string[]
) {
  const byName = new Map(fields.map((field) => [field.name, field]));
  const ordered = detailFields.flatMap((name) => byName.get(name) ? [byName.get(name)!] : []);
  const used = new Set(ordered.map((field) => field.name));
  return [
    ...ordered,
    ...fields.filter((field) => !used.has(field.name))
  ];
}

function orderedGroups(
  snapshot: FlightDeckSnapshot,
  projection: FlightDeckVisualProjection | null,
  graphView: GraphView
): string[] {
  if (!projection) {
    return [...snapshot.nodeModels].sort();
  }
  const groups = graphView === "schema"
    ? projection.nodeModels.map((hint) => hint.group)
    : snapshot.nodeModels.map((model) => schemaNodeProjection(projection, model)?.group ?? model);
  return [...new Set(groups)].sort();
}

function visualNodeShape(shape: string | undefined, schemaModel: boolean): string {
  if (schemaModel) {
    return "round-rectangle";
  }
  switch (shape) {
    case "card":
    case "lane":
      return "round-rectangle";
    case "hex":
      return "hexagon";
    case "diamond":
      return "diamond";
    default:
      return "ellipse";
  }
}

function visualNodeWidth(shape: string | undefined, detailDensity: string | undefined): number {
  if (shape === "card") {
    return detailDensity === "rich" ? 46 : 36;
  }
  if (shape === "lane") {
    return 58;
  }
  return detailDensity === "rich" ? 24 : 16;
}

function visualNodeHeight(shape: string | undefined, detailDensity: string | undefined): number {
  if (shape === "card") {
    return detailDensity === "rich" ? 28 : 22;
  }
  if (shape === "lane") {
    return 20;
  }
  return detailDensity === "rich" ? 24 : 16;
}

function visualRoleBorder(role: string | undefined): string {
  switch (role) {
    case "anchor":
      return "#ffd166";
    case "risk":
      return "#ff6a3d";
    case "decision":
      return "#d5f8ff";
    case "evidence":
      return "#89d59b";
    case "actor":
      return "#c6a8ff";
    default:
      return "#d5f8ff";
  }
}

function visualRoleOpacity(role: string | undefined): number {
  return role && role !== "generated" ? 0.88 : 0.32;
}

function visualRoleWidth(role: string | undefined): number {
  return role && role !== "generated" ? 2.2 : 1;
}

function visualEdgeLabel(model: string, isSelfLoop: boolean, projection: FlightDeckVisualProjection | null): string {
  const hint = schemaEdgeProjection(projection, model);
  const label = hint?.label ?? model;
  switch (hint?.labelVisibility) {
    case "always":
      return label;
    case "hidden":
      return "";
    case "self-loops":
      return isSelfLoop ? label : "";
    default:
      return isSelfLoop ? label : "";
  }
}

function visualEdgeWidth(weight: string | undefined, styleToken: string | undefined): number {
  if (weight === "strong" || styleToken === "warning") {
    return 2.4;
  }
  if (weight === "fine") {
    return 0.8;
  }
  return styleToken === "dependency" || styleToken === "evidence" ? 1.7 : 1;
}

function visualEdgeLineStyle(lineStyle: string | undefined, styleToken: string | undefined): string {
  if (lineStyle === "dashed" || styleToken === "dependency") {
    return "dashed";
  }
  if (lineStyle === "dotted" || styleToken === "evidence") {
    return "dotted";
  }
  return "solid";
}

function visualEdgeColor(styleToken: string | undefined): string {
  switch (styleToken) {
    case "warning":
      return "#ff6a3d";
    case "evidence":
      return "#89d59b";
    case "dependency":
      return "#ffd166";
    default:
      return "#536675";
  }
}

function visualEdgeOpacity(directionEmphasis: string | undefined): number {
  if (directionEmphasis === "strong") {
    return 0.9;
  }
  if (directionEmphasis === "muted") {
    return 0.32;
  }
  return 0.56;
}

function visualArrowScale(directionEmphasis: string | undefined): number {
  if (directionEmphasis === "strong") {
    return 0.9;
  }
  if (directionEmphasis === "muted") {
    return 0.45;
  }
  return 0.62;
}

function compactGraphLabel(label: string): string {
  const withoutModelPrefix = label.replace(/^[A-Za-z][A-Za-z0-9_]*:\s*/, "");
  if (withoutModelPrefix.length <= 34) {
    return withoutModelPrefix;
  }
  return `${withoutModelPrefix.slice(0, 31)}...`;
}

function schemaModelLabel(model: ReturnType<typeof schemaNodes>[number], visualProjection: FlightDeckVisualProjection | null): string {
  const hint = schemaNodeProjection(visualProjection, model.name);
  const orderedFields = orderSchemaFields(model.fields, hint?.detailFields ?? []);
  const fieldLines = orderedFields.length > 0
    ? orderedFields.slice(0, 7).map((field) => {
        const optional = field.required ? "" : "?";
        return `${field.name}${optional}: ${field.valueType}`;
      })
    : [`${model.idField}: id`];
  const omitted = orderedFields.length > fieldLines.length
    ? [`+${orderedFields.length - fieldLines.length} more`]
    : [];

  return [hint?.label ?? model.name, "----------", ...fieldLines, ...omitted].join("\n");
}

function schemaModelWidth(model: ReturnType<typeof schemaNodes>[number]): number {
  const longestLine = schemaModelLabel(model, null)
    .split("\n")
    .reduce((longest, line) => Math.max(longest, line.length), 0);
  return clamp(longestLine * 6.8 + 24, 140, 260);
}

function schemaModelHeight(model: ReturnType<typeof schemaNodes>[number]): number {
  const lineCount = schemaModelLabel(model, null).split("\n").length;
  return clamp(lineCount * 14.4 + 18, 64, 180);
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

function layoutOptions(layoutMode: LayoutMode, snapshot: FlightDeckSnapshot, visualProjection: FlightDeckVisualProjection | null, graphView: GraphView, graph: Core): cytoscape.LayoutOptions {
  const base = {
    animate: false,
    fit: true,
    padding: 72
  };

  switch (layoutMode) {
    case "groups": {
      const orderedModels = orderedGroups(snapshot, visualProjection, graphView);
      return {
        ...base,
        name: "concentric",
        minNodeSpacing: 48,
        concentric: (node) => {
          const group = String(node.data("group") ?? node.data("model"));
          const index = orderedModels.indexOf(group);
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
        radius: Math.max(120, graphView === "schema" ? snapshot.nodeModels.length * 11 : snapshot.nodes.length * 7),
        spacingFactor: 1.15
      };
    case "grid":
      return {
        ...base,
        name: "grid",
        avoidOverlap: true,
        nodeDimensionsIncludeLabels: true,
        avoidOverlapPadding: 24,
        condense: false
      };
    case "force":
    default:
      return {
        ...base,
        name: "cose",
        boundingBox: { x1: 0, y1: 0, w: Math.max(1, graph.width() - 144) * 2, h: Math.max(1, graph.height() - 144) * 2 },
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
