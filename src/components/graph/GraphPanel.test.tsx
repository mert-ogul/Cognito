import { GraphPanel } from "@/components/graph/GraphPanel";
import { visibleSnapshot } from "@/components/graph/GraphCyCanvas";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GraphSnapshot } from "@/lib/tauri";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("./GraphCyCanvas", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./GraphCyCanvas")>();
  return {
    ...actual,
    GraphCyCanvas: (props: {
      snapshot: GraphSnapshot | null;
      showUnconnected?: boolean;
      emptyMessage: string;
    }) => {
      const display = props.snapshot
        ? actual.visibleSnapshot(props.snapshot, !!props.showUnconnected)
        : null;
      if (!display || display.nodes.length === 0) {
        return <p>{props.emptyMessage}</p>;
      }
      return (
        <div
          data-testid="graph-canvas"
          data-node-count={display.nodes.length}
          data-show-unconnected={String(!!props.showUnconnected)}
        />
      );
    },
  };
});

const snapshotWithIsland: GraphSnapshot = {
  nodes: [
    {
      nodeId: "n1",
      name: "Ada",
      kind: "person",
      normalizedName: "ada",
      sourceCount: 2,
      confidenceAvg: 0.8,
    },
    {
      nodeId: "n2",
      name: "Meridian",
      kind: "organization",
      normalizedName: "meridian",
      sourceCount: 2,
      confidenceAvg: 0.8,
    },
    {
      nodeId: "n3",
      name: "Lonely",
      kind: "concept",
      normalizedName: "lonely",
      sourceCount: 1,
      confidenceAvg: 0.8,
    },
  ],
  edges: [
    {
      edgeId: "e1",
      sourceNodeId: "n1",
      targetNodeId: "n2",
      relation: "works at",
      documentId: "d1",
      chunkId: "c1",
      confidence: 0.8,
    },
  ],
};

describe("visibleSnapshot", () => {
  it("hides unconnected nodes by default and shows them when toggled", () => {
    const hidden = visibleSnapshot(snapshotWithIsland, false);
    expect(hidden.nodes.map((n) => n.nodeId)).toEqual(["n1", "n2"]);
    const shown = visibleSnapshot(snapshotWithIsland, true);
    expect(shown.nodes.map((n) => n.nodeId)).toEqual(["n1", "n2", "n3"]);
  });
});

describe("GraphPanel", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("shows Retry when the snapshot fails", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_graph_snapshot") {
        throw new Error("duckdb unavailable");
      }
      return {};
    });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <GraphPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText(/Couldn't load the graph/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_graph_snapshot") {
        return { nodes: [], edges: [] };
      }
      return {};
    });
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_graph_snapshot", expect.anything());
    });
  });

  it("hides unconnected nodes until Show unconnected is pressed", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_graph_snapshot") {
        return snapshotWithIsland;
      }
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          graphExtractionEnabled: true,
        };
      }
      return {};
    });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <GraphPanel />
      </QueryClientProvider>,
    );
    const canvas = await screen.findByTestId("graph-canvas");
    expect(canvas).toHaveAttribute("data-node-count", "2");
    expect(canvas).toHaveAttribute("data-show-unconnected", "false");
    fireEvent.click(screen.getByRole("button", { name: "Show unconnected" }));
    const shown = await screen.findByTestId("graph-canvas");
    expect(shown).toHaveAttribute("data-node-count", "3");
    expect(shown).toHaveAttribute("data-show-unconnected", "true");
    expect(screen.getByRole("button", { name: "Hide unconnected" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
