import { buildFocusSet, nodeFocusOpacity } from "@/components/graph/GraphCyCanvas";
import { GraphNodeDetailSheet } from "@/components/graph/GraphNodeDetailSheet";
import { e2eEmit } from "@/lib/tauri-events";
import { getDocumentPreview, getGraphNodeDetail } from "@/lib/tauri";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    getGraphNodeDetail: vi.fn(),
    getDocumentPreview: vi.fn(),
    listDocuments: vi.fn(async () => [
      {
        id: "d1",
        path: "/vault/gcn.pdf",
        title: "Semi-Supervised Classification with Graph Convolutional Networks",
        fileType: "pdf",
        contentHash: "h",
        pageCount: 10,
        status: "indexed",
        errorMessage: null,
        importedAt: "2026-01-01T00:00:00Z",
        indexedAt: "2026-01-01T01:00:00Z",
        modifiedAt: "2026-01-01T01:00:00Z",
        graphStatus: "ok",
        graphError: null,
      },
    ]),
  };
});

describe("graph focus opacity", () => {
  it("fades nodes outside the selected neighbourhood", () => {
    const focused = buildFocusSet("n1", ["n2"]);
    expect(nodeFocusOpacity(focused, "n1")).toBe(1);
    expect(nodeFocusOpacity(focused, "n2")).toBe(1);
    expect(nodeFocusOpacity(focused, "n3")).toBe(0.15);
    expect(nodeFocusOpacity(null, "n3")).toBe(1);
  });
});

describe("GraphNodeDetailSheet source link", () => {
  beforeEach(() => {
    useSidePaneStore.setState({
      open: false,
      mode: "graph",
      passage: null,
      highlightNodeIds: [],
    });
    vi.mocked(getGraphNodeDetail).mockResolvedValue({
      node: {
        nodeId: "n1",
        name: "Graph attention network",
        kind: "method",
        sourceCount: 2,
      },
      edges: [
        {
          edgeId: "e1",
          sourceNodeId: "n1",
          targetNodeId: "n2",
          relation: "extends",
          documentId: "d1",
          chunkId: "c9",
          confidence: 0.8,
          sourceName: "Graph attention network",
          targetName: "Graph convolutional network",
        },
      ],
    });
    vi.mocked(getDocumentPreview).mockResolvedValue({
      documentId: "d1",
      text: "cited passage body",
      highlightSnippet: "cited passage body",
      pageStart: 4,
      pageEnd: 4,
    });
  });

  it("opens the Passage pane from a relation source link", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <GraphNodeDetailSheet nodeId="n1" open onClose={() => {}} />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/Graph attention network, extends, Graph convolutional network/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Source" }));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("d1", "c9");
    });
    await waitFor(() => {
      expect(useSidePaneStore.getState().mode).toBe("passage");
      expect(useSidePaneStore.getState().passage?.documentId).toBe("d1");
      expect(useSidePaneStore.getState().passage?.highlight).toBe("cited passage body");
    });
    void e2eEmit;
  });
});
