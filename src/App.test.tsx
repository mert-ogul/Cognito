import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_app_health") {
      return {
        sqliteOk: true,
        sidecarReachable: true,
        ollamaModelReady: true,
        vaultConfigured: false,
        chatModel: "qwen3.5:9b",
        embedModel: "qwen3-embedding:0.6b",
        chatModelWarmed: true,
        warmUpPending: false,
        warmUpError: null,
        ollamaReachable: true,
        missingModels: [],
        indexFormatUpdating: false,
        checkingModelsReady: true,
      };
    }
    if (cmd === "list_documents") {
      return [];
    }
    if (cmd === "list_conversations") {
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
        chatModel: "qwen3.5:9b",
        embedModel: "qwen3-embedding:0.6b",
        backgroundModel: "qwen3.5:9b",
        summariseDocuments: true,
        extractEntities: false,
      extraEntityLabels: "",
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
    expect(screen.getByRole("img", { name: "Cognito" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Library", pressed: true })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Chats" })).toBeInTheDocument();
    expect(screen.getByLabelText("Chat message")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connections" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open settings" })).toBeInTheDocument();
  });
});
