import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "./AppShell";
import { useMainViewStore } from "@/stores/mainViewStore";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    getAppHealth: vi.fn(async () => ({
      sqliteOk: true,
      sidecarReachable: true,
      ollamaModelReady: true,
      vaultConfigured: true,
      chatModel: "qwen3.5:9b",
      embedModel: "qwen3-embedding:0.6b",
      chatModelWarmed: true,
      warmUpPending: false,
      warmUpError: null,
      ollamaReachable: true,
      missingModels: [],
      indexFormatUpdating: false,
      checkingModelsReady: true,
    })),
    listDocuments: vi.fn(async () => [
      {
        id: "d1",
        path: "/vault/a.pdf",
        title: "Alpha Paper",
        fileType: "pdf",
        contentHash: "h",
        pageCount: 3,
        status: "indexed",
        errorMessage: null,
        importedAt: "2026-01-01T00:00:00Z",
        indexedAt: "2026-01-01T01:00:00Z",
        modifiedAt: "2026-01-01T01:00:00Z",
        entitiesStatus: "ok",
        entitiesError: null,
        cardStatus: "ok",
        cardType: "paper",
        cardDate: null,
        cardPeople: [],
        cardOrganizations: [],
      },
    ]),
    getSettings: vi.fn(async () => ({
      vaultPath: "/vault",
      chatModel: "qwen3.5:9b",
      embedModel: "qwen3-embedding:0.6b",
      backgroundModel: "qwen3.5:9b",
      summariseDocuments: false,
      extractEntities: false,
      extraEntityLabels: "",
    })),
    getDocumentCard: vi.fn(async () => null),
    getSuggestedQuestions: vi.fn(async () => []),
    sendChatQuery: vi.fn(async () => "mid"),
    warmChatModel: vi.fn(async () => {}),
  };
});

describe("AppShell conversation persistence", () => {
  beforeEach(() => {
    useMainViewStore.setState({ view: "conversation", documentId: null });
  });

  it("keeps chat draft after opening a document and going back", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <AppShell />
      </QueryClientProvider>,
    );

    const input = await screen.findByLabelText("Chat message");
    await waitFor(() => {
      expect(input).not.toBeDisabled();
    });
    fireEvent.change(input, { target: { value: "draft about GCN" } });
    expect(screen.getByDisplayValue("draft about GCN")).toBeInTheDocument();

    fireEvent.click(await screen.findByText("Alpha Paper"));
    expect(await screen.findByRole("button", { name: "Conversation" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("draft about GCN")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Conversation" }));
    expect(screen.getByDisplayValue("draft about GCN")).toBeInTheDocument();
  });
});
