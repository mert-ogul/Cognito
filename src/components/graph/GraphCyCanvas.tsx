import type { GraphSnapshot } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import cytoscape, { type Core } from "cytoscape";
import fcose from "cytoscape-fcose";
import { useEffect, useMemo, useRef } from "react";

cytoscape.use(fcose);

type Props = {
  snapshot: GraphSnapshot | null;
  highlightNodeIds: string[];
  selectedNodeId: string | null;
  onNodeTap: (nodeId: string) => void;
  onBackgroundTap?: () => void;
  fitKey?: number;
  centerOnNodeId?: string | null;
  className?: string;
  emptyMessage: string;
  showUnconnected?: boolean;
};

type Theme = {
  text: string;
  muted: string;
  line: string;
  accent: string;
  pane: string;
};

function readTheme(): Theme {
  const s = getComputedStyle(document.documentElement);
  const pick = (name: string) => s.getPropertyValue(name).trim();
  return {
    text: pick("--text"),
    muted: pick("--text-muted"),
    line: pick("--line"),
    accent: pick("--accent"),
    pane: pick("--bg-pane"),
  };
}

function connectedNodeIds(snapshot: GraphSnapshot): Set<string> {
  const ids = new Set<string>();
  for (const e of snapshot.edges) {
    ids.add(e.sourceNodeId);
    ids.add(e.targetNodeId);
  }
  return ids;
}

/** Drop nodes with no edges in the current snapshot unless showUnconnected is set. */
export function visibleSnapshot(
  snapshot: GraphSnapshot,
  showUnconnected: boolean,
): GraphSnapshot {
  if (showUnconnected) return snapshot;
  const connected = connectedNodeIds(snapshot);
  return {
    nodes: snapshot.nodes.filter((n) => connected.has(n.nodeId)),
    edges: snapshot.edges,
  };
}

function degreeByNode(snapshot: GraphSnapshot): Map<string, number> {
  const deg = new Map<string, number>();
  for (const n of snapshot.nodes) deg.set(n.nodeId, 0);
  for (const e of snapshot.edges) {
    deg.set(e.sourceNodeId, (deg.get(e.sourceNodeId) ?? 0) + 1);
    deg.set(e.targetNodeId, (deg.get(e.targetNodeId) ?? 0) + 1);
  }
  return deg;
}

function buildElements(snapshot: GraphSnapshot) {
  const deg = degreeByNode(snapshot);
  const maxDeg = Math.max(1, ...deg.values());
  const nodes = snapshot.nodes.map((n) => {
    const isWork = n.kind === "work";
    const d = deg.get(n.nodeId) ?? 0;
    const size = isWork ? 24 : 8 + (8 * d) / maxDeg;
    const showLabel = isWork || d >= 3 ? 1 : 0;
    return {
      data: {
        id: n.nodeId,
        label: n.name,
        kind: n.kind,
        degree: d,
        size,
        showLabel,
        isWork: isWork ? 1 : 0,
      },
    };
  });
  const edges = snapshot.edges.map((e) => ({
    data: {
      id: e.edgeId,
      source: e.sourceNodeId,
      target: e.targetNodeId,
    },
  }));
  return [...nodes, ...edges];
}

function applyThemeStyles(cy: Core, theme: Theme) {
  cy.style()
    .selector("node")
    .style({
      label: "data(label)",
      "font-family": "var(--family-ui)",
      "font-size": "11px",
      "text-wrap": "wrap",
      "text-max-width": "90px",
      "text-valign": "bottom",
      "text-margin-y": 4,
      color: theme.text,
      "background-color": theme.pane,
      "border-width": 1.5,
      "border-color": theme.line,
      width: "data(size)",
      height: "data(size)",
      "min-zoomed-font-size": 13,
      opacity: 1,
    })
    .selector("node[showLabel = 1]")
    .style({
      "min-zoomed-font-size": 0,
    })
    .selector("node[isWork = 1]")
    .style({
      "border-color": theme.accent,
      "border-width": 2.5,
      "min-zoomed-font-size": 0,
    })
    .selector("edge")
    .style({
      width: 1,
      "line-color": theme.line,
      "curve-style": "bezier",
      "target-arrow-shape": "none",
      "source-arrow-shape": "none",
      opacity: 1,
    })
    .update();
}

/** Opacity for a node under focus: selected + neighbours stay at 1, others fade. */
export function nodeFocusOpacity(
  focused: Set<string> | null,
  nodeId: string,
): number {
  if (!focused) return 1;
  return focused.has(nodeId) ? 1 : 0.15;
}

export function buildFocusSet(
  selectedNodeId: string | null,
  neighborIds: string[],
): Set<string> | null {
  if (!selectedNodeId) return null;
  return new Set([selectedNodeId, ...neighborIds]);
}

function applyHighlight(
  cy: Core,
  highlightNodeIds: string[],
  selectedNodeId: string | null,
  theme: Theme,
) {
  const hl = new Set(highlightNodeIds);
  let focused: Set<string> | null = null;
  if (selectedNodeId) {
    const n = cy.getElementById(selectedNodeId);
    const neighbors: string[] = [];
    if (n.nonempty()) {
      n.neighborhood("node").forEach((nb) => {
        neighbors.push(nb.id());
      });
    }
    focused = buildFocusSet(selectedNodeId, neighbors);
  }
  const zoom = cy.zoom();
  cy.batch(() => {
    cy.nodes().forEach((node) => {
      const id = node.id();
      const isHl = hl.has(id);
      const isSel = selectedNodeId === id;
      const isWork = node.data("isWork") === 1;
      const deg = Number(node.data("degree") || 0);
      node.style("background-color", isHl ? theme.accent : theme.pane);
      node.style(
        "border-color",
        isSel || isWork ? theme.accent : theme.line,
      );
      node.style("border-width", isSel ? 3 : isWork ? 2.5 : 1.5);
      node.style("color", theme.text);
      node.style("opacity", nodeFocusOpacity(focused, id));
      const show =
        isWork || deg >= 3 || zoom > 1.2 || node.data("showLabel") === 1;
      node.style("min-zoomed-font-size", show ? 0 : 13);
    });
    cy.edges().forEach((edge) => {
      if (!focused) {
        edge.style("opacity", 1);
        return;
      }
      const s = edge.source().id();
      const t = edge.target().id();
      edge.style(
        "opacity",
        focused.has(s) && focused.has(t) ? 1 : 0.15,
      );
    });
  });
}

function fitFocused(cy: Core, selectedNodeId: string | null) {
  if (!selectedNodeId) {
    cy.fit(cy.elements(), 24);
    return;
  }
  const n = cy.getElementById(selectedNodeId);
  if (!n.nonempty()) {
    cy.fit(cy.elements(), 24);
    return;
  }
  const set = n.union(n.neighborhood());
  cy.fit(set, 40);
}

export function GraphCyCanvas({
  snapshot,
  highlightNodeIds,
  selectedNodeId,
  onNodeTap,
  onBackgroundTap,
  fitKey = 0,
  centerOnNodeId,
  className,
  emptyMessage,
  showUnconnected = false,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Core | null>(null);
  const onBgRef = useRef(onBackgroundTap);
  onBgRef.current = onBackgroundTap;
  const display = useMemo(
    () => (snapshot ? visibleSnapshot(snapshot, showUnconnected) : null),
    [snapshot, showUnconnected],
  );

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !display || display.nodes.length === 0) {
      return;
    }
    const theme = readTheme();
    const cy = cytoscape({
      container: el,
      elements: buildElements(display),
      style: [],
      layout: {
        name: "fcose",
        animate: false,
        padding: 24,
        packComponents: true,
        nodeRepulsion: 8000,
        idealEdgeLength: 90,
      } as cytoscape.LayoutOptions,
      wheelSensitivity: 0.35,
    });
    applyThemeStyles(cy, theme);
    cy.userPanningEnabled(true);
    cy.userZoomingEnabled(true);
    cy.boxSelectionEnabled(false);
    cy.on("tap", "node", (evt) => {
      onNodeTap(evt.target.id());
    });
    cy.on("tap", (evt) => {
      if (evt.target === cy) {
        onBgRef.current?.();
      }
    });
    cy.on("zoom", () => {
      applyHighlight(cy, highlightNodeIds, selectedNodeId, readTheme());
    });
    cyRef.current = cy;
    applyHighlight(cy, highlightNodeIds, selectedNodeId, theme);

    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onScheme = () => {
      const next = readTheme();
      applyThemeStyles(cy, next);
      applyHighlight(cy, highlightNodeIds, selectedNodeId, next);
    };
    mq.addEventListener("change", onScheme);

    return () => {
      mq.removeEventListener("change", onScheme);
      cy.destroy();
      cyRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- highlight updates via the effect below
  }, [display, onNodeTap]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    applyHighlight(cy, highlightNodeIds, selectedNodeId, readTheme());
  }, [highlightNodeIds, selectedNodeId, display]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy || fitKey === 0) return;
    fitFocused(cy, selectedNodeId);
  }, [fitKey, selectedNodeId]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy || !centerOnNodeId) return;
    const n = cy.getElementById(centerOnNodeId);
    if (n.nonempty()) {
      fitFocused(cy, centerOnNodeId);
    }
  }, [centerOnNodeId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onBgRef.current?.();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!display || display.nodes.length === 0) {
    return (
      <div className={cn("flex min-h-[200px] items-center justify-center p-3 text-muted", className)}>
        {emptyMessage}
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      data-testid="graph-canvas"
      className={cn("min-h-[280px] w-full flex-1 bg-pane", className)}
    />
  );
}
