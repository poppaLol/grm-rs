import type { VisualProjectionContainerOverlay } from "./projectionOverlay";
import type { FlightDeckSnapshot } from "./types";

export interface VisualContainerRegion {
  id: string;
  anchorId: string;
  memberIds: string[];
  renderAs: string;
  collapsed: boolean;
  styleToken?: string;
}

export interface VisualContainerMap {
  regions: VisualContainerRegion[];
  ownerByNode: Map<string, string>;
}

export function buildVisualContainerMap(
  snapshot: FlightDeckSnapshot,
  rules: Record<string, VisualProjectionContainerOverlay>
): VisualContainerMap {
  const nodes = new Map(snapshot.nodes.map((node) => [node.id, node]));
  const candidates = new Map<string, { rule: VisualProjectionContainerOverlay; children: Set<string> }>();
  const edges = [...snapshot.edges].sort((a, b) => a.from.localeCompare(b.from) || a.to.localeCompare(b.to) || a.id.localeCompare(b.id));
  for (const [, rule] of Object.entries(rules).sort(([a], [b]) => a.localeCompare(b))) {
    for (const edge of edges) {
      if (edge.model !== rule.viaEdgeModel || edge.from === edge.to
        || nodes.get(edge.from)?.model !== rule.parentModel
        || nodes.get(edge.to)?.model !== rule.childModel) {
        continue;
      }
      const candidate = candidates.get(edge.from) ?? { rule, children: new Set<string>() };
      candidate.children.add(edge.to);
      candidates.set(edge.from, candidate);
    }
  }

  // Flat regions reserve anchors first. Shared children use the first stable owner;
  // other relationships remain links rather than implying nested containment.
  const ownerByNode = new Map<string, string>();
  const regions: VisualContainerRegion[] = [];
  const reservedIds = new Set(nodes.keys());
  for (const [anchorId, { rule, children }] of [...candidates].sort(([a], [b]) => a.localeCompare(b))) {
    const memberIds = [...children].filter((id) => !candidates.has(id) && !ownerByNode.has(id)).sort();
    if (memberIds.length === 0) {
      continue;
    }
    let id = `visual-container:${anchorId}`;
    while (reservedIds.has(id)) {
      id = `visual-container:${id}`;
    }
    reservedIds.add(id);
    const region: VisualContainerRegion = {
      id, anchorId, memberIds,
      renderAs: rule.renderAs ?? "region",
      collapsed: rule.collapse === "collapsed",
      styleToken: rule.styleToken
    };
    regions.push(region);
    ownerByNode.set(anchorId, id);
    for (const memberId of memberIds) {
      ownerByNode.set(memberId, id);
    }
  }
  return { regions, ownerByNode };
}
