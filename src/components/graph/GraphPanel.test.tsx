import { GraphPanel } from "@/components/graph/GraphPanel";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

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
});
