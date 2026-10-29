import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("@/lib/tauri-events", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_app_health") {
      return {
        sqliteOk: true,
        duckdbOk: true,
        sidecarReachable: true,
        ollamaModelReady: true,
        vaultConfigured: false,
        chatModel: "phi4-mini",
        embedModel: "nomic-embed-text",
        chatModelWarmed: true,
        warmUpPending: false,
        warmUpError: null,
        ollamaReachable: true,
        missingModels: [],
      };
    }
    if (cmd === "list_documents") {
      return [];
    }
    if (cmd === "get_graph_snapshot") {
      return { nodes: [], edges: [] };
    }
    if (cmd === "search_graph") {
      return [];
    }
    if (cmd === "get_graph_node_detail") {
      return null;
    }
    if (cmd === "get_settings") {
      return {
        vaultPath: null,
        chatModel: "phi4-mini",
        embedModel: "nomic-embed-text",
        graphExtractionEnabled: true,
      };
    }
    return {};
  }),
}));

describe("App shell", () => {
  it("renders Cognito layout regions", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>,
    );
    expect(screen.getByText("Cognito")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Sources" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Chat" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Graph" })).toBeInTheDocument();
  });
});
