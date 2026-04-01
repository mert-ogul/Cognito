import type { GraphSnapshot } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import cytoscape, { type Core } from "cytoscape";
import { useEffect, useRef } from "react";

type Props = {
  snapshot: GraphSnapshot | null;
  highlightNodeIds: string[];
  selectedNodeId: string | null;
  onNodeTap: (nodeId: string) => void;
  /** Increment to run `cy.fit()`. */
  fitKey?: number;
  /** Center viewport on this node id when it changes. */
  centerOnNodeId?: string | null;
  className?: string;
};

function buildElements(snapshot: GraphSnapshot) {
  const nodes = snapshot.nodes.map((n) => ({
    data: { id: n.nodeId, label: n.name, kind: n.kind },
  }));
  const edges = snapshot.edges.map((e) => ({
    data: {
      id: e.edgeId,
      source: e.sourceNodeId,
      target: e.targetNodeId,
      label: e.relation,
    },
  }));
  return [...nodes, ...edges];
}

function applyHighlight(cy: Core, highlightNodeIds: string[], selectedNodeId: string | null) {
  const hl = new Set(highlightNodeIds);
  cy.batch(() => {
    cy.nodes().forEach((n) => {
      const id = n.id();
      const isHl = hl.has(id);
      const isSel = selectedNodeId === id;
      n.style("border-color", isHl ? "#fbbf24" : isSel ? "#38bdf8" : "#334155");
      n.style("background-color", isHl ? "#422006" : "#1e293b");
      n.style("border-width", isHl || isSel ? 3 : 2);
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
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Core | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !snapshot || snapshot.nodes.length === 0) {
      return;
    }
    const cy = cytoscape({
      container: el,
      elements: buildElements(snapshot),
      style: [
        {
          selector: "node",
          style: {
            label: "data(label)",
            "font-size": "10px",
            "text-wrap": "wrap",
            "text-max-width": "100px",
            color: "#e2e8f0",
            "background-color": "#1e293b",
            width: 28,
            height: 28,
            "border-width": 2,
            "border-color": "#334155",
          },
        },
        {
          selector: "edge",
          style: {
            width: 1.5,
            "line-color": "#475569",
            "target-arrow-color": "#475569",
            "curve-style": "bezier",
            "target-arrow-shape": "triangle",
            opacity: 0.85,
          },
        },
      ],
      layout: { name: "circle", padding: 24 } as cytoscape.LayoutOptions,
      wheelSensitivity: 0.35,
    });
    cy.userPanningEnabled(true);
    cy.userZoomingEnabled(true);
    cy.boxSelectionEnabled(false);
    cy.on("tap", "node", (evt) => {
      onNodeTap(evt.target.id());
    });
    cyRef.current = cy;
    applyHighlight(cy, highlightNodeIds, selectedNodeId);
    return () => {
      cy.destroy();
      cyRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- highlight updates via the effect below
  }, [snapshot, onNodeTap]);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    applyHighlight(cy, highlightNodeIds, selectedNodeId);
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
      cy.animate({ center: { eles: n }, zoom: Math.max(cy.zoom(), 1) }, { duration: 200 });
    }
  }, [centerOnNodeId]);

  if (!snapshot || snapshot.nodes.length === 0) {
    return (
      <div
        className={cn(
          "flex min-h-[200px] items-center justify-center rounded-md border border-dashed border-[var(--border)] text-xs text-[var(--muted)]",
          className,
        )}
      >
        No graph nodes yet. Index documents with graph extraction enabled.
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      data-testid="graph-canvas"
      className={cn("min-h-[280px] w-full flex-1 rounded-md border border-[var(--border)] bg-[#0f172a]", className)}
    />
  );
}
