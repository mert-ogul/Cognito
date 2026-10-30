import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DocumentPage } from "./DocumentPage";
import {
  getDocumentCard,
  getDocumentPreview,
  getSettings,
  listDocuments,
  openDocument,
  renderDocumentPage,
  TINY_PNG_BASE64,
} from "@/lib/tauri";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    getDocumentCard: vi.fn(),
    getDocumentPreview: vi.fn(),
    getSettings: vi.fn(),
    listDocuments: vi.fn(),
    openDocument: vi.fn(async () => {}),
    renderDocumentPage: vi.fn(),
  };
});

describe("DocumentPage", () => {
  beforeEach(() => {
    useSidePaneStore.setState({
      open: false,
      mode: "connections",
      passage: null,
      highlightNodeIds: [],
    });
    useCitedRegionsStore.setState({ byDoc: {} });
    useChatIntentStore.setState({
      pendingScope: [],
      pendingQuestion: null,
      pendingRelated: null,
    });
    vi.mocked(renderDocumentPage).mockResolvedValue({
      pngBase64: TINY_PNG_BASE64,
      width: 100,
      height: 140,
    });
    vi.mocked(listDocuments).mockResolvedValue([
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
        cardDate: "2024-06-01",
        cardPeople: ["Ada Chen"],
        cardOrganizations: ["Meridian Labs"],
      },
    ]);
    vi.mocked(getDocumentCard).mockResolvedValue({
      documentId: "d1",
      type: "paper",
      title: "Alpha Paper",
      date: "2024-06-01",
      people: [{ name: "Ada Chen", role: "author" }],
      organizations: ["Meridian Labs"],
      summary: [
        {
          text: "This paper introduces graph convolutions.",
          quotes: [{ source: 1, text: "graph convolutions" }],
        },
      ],
      facts: [
        {
          text: "Ada Chen works at Meridian Labs.",
          quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
        },
      ],
      fields: [{ key: "venue", value: "ICLR", quotes: [{ source: 1, text: "ICLR" }] }],
      aliases: [],
      questions: [],
      model: "qwen3.5:4b",
      createdAt: "2026-01-01T02:00:00Z",
    });
    vi.mocked(getDocumentPreview).mockResolvedValue({
      documentId: "d1",
      text: "Ada Chen works at Meridian Labs.",
      highlightSnippet: null,
      pageStart: 2,
      pageEnd: 2,
    });
    vi.mocked(getSettings).mockResolvedValue({
      vaultPath: "/vault",
      chatModel: "qwen3.5:9b",
      embedModel: "qwen3-embedding:0.6b",
      backgroundModel: "qwen3.5:4b",
      summariseDocuments: true,
      extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
    });
  });

  it("renders card summary, facts with source links, and fields", async () => {
    const onBack = vi.fn();
    const onAsk = vi.fn();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <DocumentPage documentId="d1" onBackToConversation={onBack} onAskAboutDocument={onAsk} />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Alpha Paper")).toBeInTheDocument();
    expect(screen.getByText(/paper · 2024-06-01/)).toBeInTheDocument();
    expect(screen.getByText(/People: Ada Chen/)).toBeInTheDocument();
    expect(screen.getByText("This paper introduces graph convolutions.")).toBeInTheDocument();
    expect(screen.getByText("Ada Chen works at Meridian Labs.")).toBeInTheDocument();
    expect(screen.getByText("venue")).toBeInTheDocument();
    expect(screen.getByText("ICLR")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "source" }));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("d1", null);
    });
    expect(useSidePaneStore.getState().passage).toMatchObject({
      documentId: "d1",
      highlight: "Ada Chen works at Meridian Labs.",
      text: "Ada Chen works at Meridian Labs.",
      pageLabel: "p. 2",
    });
  });

  it("Ask about this document sets scope and goes back", async () => {
    const onBack = vi.fn();
    const onAsk = vi.fn();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <DocumentPage documentId="d1" onBackToConversation={onBack} onAskAboutDocument={onAsk} />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Alpha Paper")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Ask about this document" }));
    expect(onAsk).toHaveBeenCalledWith({ id: "d1", title: "Alpha Paper" });
    expect(onBack).toHaveBeenCalled();
    expect(useChatIntentStore.getState().pendingScope).toEqual([
      { id: "d1", title: "Alpha Paper" },
    ]);
  });

  it("shows no summary yet when summarise is off even if card is pending", async () => {
    vi.mocked(getSettings).mockResolvedValue({
      vaultPath: "/vault",
      chatModel: "qwen3.5:9b",
      embedModel: "qwen3-embedding:0.6b",
      backgroundModel: "qwen3.5:4b",
      summariseDocuments: false,
      extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
    });
    vi.mocked(listDocuments).mockResolvedValue([
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
        cardStatus: "pending",
        cardType: null,
        cardDate: null,
        cardPeople: [],
        cardOrganizations: [],
      },
    ]);
    vi.mocked(getDocumentCard).mockResolvedValue(null);
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <DocumentPage
          documentId="d1"
          onBackToConversation={() => {}}
          onAskAboutDocument={() => {}}
        />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("No summary yet.")).toBeInTheDocument();
    expect(screen.queryByText("Summarising…")).not.toBeInTheDocument();
  });

  it("Open file calls openDocument", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <DocumentPage
          documentId="d1"
          onBackToConversation={() => {}}
          onAskAboutDocument={() => {}}
        />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Open file" }));
    await waitFor(() => {
      expect(openDocument).toHaveBeenCalledWith("d1");
    });
  });
});
