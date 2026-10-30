import { e2eEmit } from "@/lib/tauri-events";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LibraryPanel } from "./LibraryPanel";
import { useMainViewStore } from "@/stores/mainViewStore";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function baseDoc(overrides: Record<string, unknown> = {}) {
  return {
    id: "1",
    path: "/vault/a.pdf",
    title: "Alpha Paper",
    fileType: "pdf",
    contentHash: "abc",
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
    ...overrides,
  };
}

describe("LibraryPanel", () => {
  beforeEach(() => {
    useMainViewStore.setState({ view: "conversation", documentId: null });
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          baseDoc(),
          baseDoc({
            id: "2",
            path: "/vault/b.md",
            title: "Notes on GAT",
            fileType: "markdown",
            pageCount: null,
            status: "failed",
            errorMessage: "boom",
            importedAt: "2026-01-02T00:00:00Z",
            indexedAt: null,
            modifiedAt: "2026-01-02T00:00:00Z",
            entitiesStatus: "none",
            cardStatus: "none",
            cardType: null,
            cardDate: null,
            cardPeople: [],
            cardOrganizations: [],
          }),
        ];
      }
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      return {};
    });
  });

  it("renders document list with card line and failed state", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Alpha Paper")).toBeInTheDocument();
    expect(screen.getByText(/paper · 2024-06-01 · Ada Chen, Meridian Labs · 3 pages/)).toBeInTheDocument();
    expect(screen.getByText("Notes on GAT")).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();
  });

  it("sorts by title and filters by type", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          baseDoc({
            id: "1",
            title: "Zebra",
            cardType: "paper",
            cardDate: "2020-01-01",
            pageCount: 10,
            cardPeople: [],
            cardOrganizations: [],
          }),
          baseDoc({
            id: "2",
            path: "/vault/b.md",
            title: "Alpha",
            cardType: "note",
            cardDate: "2025-01-01",
            pageCount: 2,
            cardPeople: [],
            cardOrganizations: [],
          }),
        ];
      }
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      return {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    await screen.findByText("Zebra");
    const titles = () =>
      screen.getAllByRole("button").filter((b) => /Zebra|Alpha/.test(b.textContent ?? ""));
    // default sort title: Alpha then Zebra
    let rows = titles();
    expect(rows[0]).toHaveTextContent("Alpha");
    expect(rows[1]).toHaveTextContent("Zebra");

    fireEvent.click(screen.getByRole("button", { name: "type" }));
    rows = titles();
    expect(rows[0]).toHaveTextContent("Alpha");
    expect(rows[1]).toHaveTextContent("Zebra");

    fireEvent.click(screen.getByRole("button", { name: "note" }));
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Zebra")).not.toBeInTheDocument();
  });

  it("selects a document on row click", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByText("Alpha Paper"));
    expect(useMainViewStore.getState()).toMatchObject({
      view: "document",
      documentId: "1",
    });
  });

  it("shows import progress when import-progress event fires", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    await screen.findByText("Alpha Paper");
    act(() => {
      e2eEmit("import-progress", {
        jobId: "j1",
        status: "running",
        totalFiles: 10,
        completedFiles: 4,
        failedFiles: 0,
        phase: "parse",
        path: "/vault/x.pdf",
      });
    });
    expect(await screen.findByText(/Indexing 4 of 10, x\.pdf/)).toBeInTheDocument();
  });

  it("shows summarising footer for pending cards", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          baseDoc({ id: "1", cardStatus: "ok" }),
          baseDoc({
            id: "2",
            path: "/vault/b.pdf",
            title: "Beta",
            cardStatus: "pending",
            cardType: "paper",
            cardPeople: [],
            cardOrganizations: [],
          }),
        ];
      }
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      return {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Summarising 1 of 2")).toBeInTheDocument();
    expect(screen.getByText("Summarising")).toBeInTheDocument();
  });

  it("does not show summarising when summarise setting is off", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          baseDoc({
            id: "2",
            path: "/vault/b.pdf",
            title: "Beta",
            cardStatus: "pending",
            cardType: "paper",
            cardPeople: [],
            cardOrganizations: [],
          }),
        ];
      }
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: false,
          extractEntities: false,
          extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      return {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Beta")).toBeInTheDocument();
    expect(screen.queryByText("Summarising")).not.toBeInTheDocument();
    expect(screen.queryByText(/Summarising \d+ of/)).not.toBeInTheDocument();
  });

  it("shows empty-state guidance when there is no vault", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") return [];
      if (cmd === "get_settings") {
        return {
          vaultPath: null,
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      return {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(
        /Pick a folder of PDF, Word and Markdown files\. Cognito indexes it on this computer\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose a folder" })).toBeInTheDocument();
  });

  it("empty-state Choose a folder calls setVault", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") return [];
      if (cmd === "get_settings") {
        return {
          vaultPath: null,
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      if (cmd === "select_vault_folder") return "/chosen";
      if (cmd === "set_vault") return "job-1";
      return {};
    });
    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Choose a folder" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("select_vault_folder");
      expect(invokeMock).toHaveBeenCalledWith("set_vault", { path: "/chosen" });
    });
  });

  it("shows quiet graph waiting when graph is enabled", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          baseDoc({ entitiesStatus: "pending", cardStatus: "ok" }),
          baseDoc({
            id: "2",
            path: "/vault/b.md",
            title: "b",
            entitiesStatus: "running",
            cardStatus: "ok",
            cardPeople: [],
            cardOrganizations: [],
          }),
        ];
      }
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: true,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      return {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Finding names, 2 files left")).toBeInTheDocument();
  });

  it("shows rejected import error with role alert", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") return [];
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          backgroundModel: "qwen3.5:4b",
          summariseDocuments: true,
          extractEntities: false,
      extraEntityLabels: "",
      thoroughReranking: false,
        };
      }
      if (cmd === "import_documents") {
        throw "No PDF, Word or Markdown files found.";
      }
      return {};
    });
    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <LibraryPanel />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No PDF, Word or Markdown files found.",
    );
  });
});
