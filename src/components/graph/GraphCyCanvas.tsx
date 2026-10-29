import type { GraphSnapshot } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import cytoscape, { type Core } from "cytoscape";
import { useEffect, useRef } from "react";

type Props = {
  snapshot: GraphSnapshot | null;
  highlightNodeIds: string[];
  selectedNodeId: string | null;
  onNodeTap: (nodeId: string) => void;
  fitKey?: number;
  centerOnNodeId?: string | null;
  className?: string;
  emptyMessage: string;
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

function buildElements(snapshot: GraphSnapshot) {
  const maxSources = Math.max(1, ...snapshot.nodes.map((n) => n.sourceCount || 1));
  const ranked = [...snapshot.nodes].sort(
    (a, b) => (b.sourceCount || 0) - (a.sourceCount || 0),
  );
  const labeled = new Set(ranked.slice(0, 30).map((n) => n.nodeId));

  const nodes = snapshot.nodes.map((n) => {
    const count = n.sourceCount || 1;
    const size = 8 + (12 * count) / maxSources;
    return {
      data: {
        id: n.nodeId,
        label: n.name,
        kind: n.kind,
        sourceCount: count,
        size,
        showLabel: labeled.has(n.nodeId) ? 1 : 0,
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
    })
    .selector("node[showLabel = 1]")
    .style({
      "min-zoomed-font-size": 0,
    })
    .selector("edge")
    .style({
      width: 1,
      "line-color": theme.line,
      "curve-style": "bezier",
      "target-arrow-shape": "none",
      "source-arrow-shape": "none",
    })
    .update();
}

function applyHighlight(cy: Core, highlightNodeIds: string[], selectedNodeId: string | null, theme: Theme) {
  const hl = new Set(highlightNodeIds);
  cy.batch(() => {
    cy.nodes().forEach((n) => {
      const id = n.id();
      const isHl = hl.has(id);
      const isSel = selectedNodeId === id;
      n.style("background-color", isHl ? theme.accent : theme.pane);
      n.style("border-color", isSel ? theme.accent : theme.line);
      n.style("border-width", isSel ? 3 : 1.5);
      n.style("color", theme.text);
    });
  });
}

export function GraphCyCanvas({
  snapshot,
  highlightNodeIds,
  selectedNodeId,
  onNodeTap,
  fitKey = 0,
  centerOnNodeId,
  className,
  emptyMessage,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Core | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !snapshot || snapshot.nodes.length === 0) {
      return;
    }
    const theme = readTheme();
    const cy = cytoscape({
      container: el,
      elements: buildElements(snapshot),
      style: [],
      layout: {
        name: "cose",
        animate: false,
        padding: 24,
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
  }, [snapshot, onNodeTap]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    applyHighlight(cy, highlightNodeIds, selectedNodeId, readTheme());
  }, [highlightNodeIds, selectedNodeId, snapshot]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy || fitKey === 0) return;
    cy.fit(cy.elements(), 24);
  }, [fitKey]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy || !centerOnNodeId) return;
    const n = cy.getElementById(centerOnNodeId);
    if (n.nonempty()) {
      cy.center(n);
      if (cy.zoom() < 1) {
        cy.zoom(1);
      }
    }
  }, [centerOnNodeId]);

  if (!snapshot || snapshot.nodes.length === 0) {
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
