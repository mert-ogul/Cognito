import { e2eEmit } from "@/lib/tauri-events";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SourcesPanel } from "./SourcesPanel";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe("SourcesPanel", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          {
            id: "1",
            path: "/vault/a.pdf",
            title: "a",
            fileType: "pdf",
            contentHash: "abc",
            pageCount: 3,
            status: "indexed",
            errorMessage: null,
            importedAt: "2026-01-01T00:00:00Z",
            indexedAt: "2026-01-01T01:00:00Z",
            modifiedAt: "2026-01-01T01:00:00Z",
            graphStatus: "ok",
            graphError: null,
          },
          {
            id: "2",
            path: "/vault/b.md",
            title: "b",
            fileType: "markdown",
            contentHash: "def",
            pageCount: null,
            status: "failed",
            errorMessage: "boom",
            importedAt: "2026-01-02T00:00:00Z",
            indexedAt: null,
            modifiedAt: "2026-01-02T00:00:00Z",
            graphStatus: "none",
            graphError: null,
          },
        ];
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
  });

  it("renders document list with type lines and failed state", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("a.pdf")).toBeInTheDocument();
    expect(screen.getByText("PDF, 3 pages")).toBeInTheDocument();
    expect(screen.getByText("b.md")).toBeInTheDocument();
    expect(screen.getByText("Markdown")).toBeInTheDocument();
    expect(screen.getByText("Couldn't read this file")).toBeInTheDocument();
  });

  it("shows import progress when import-progress event fires", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    await screen.findByText("a.pdf");
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

  it("shows empty-state guidance when there is no vault", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") return [];
      if (cmd === "get_settings") {
        return {
          vaultPath: null,
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          graphExtractionEnabled: true,
        };
      }
      return {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(
        /Pick a folder of PDF and Markdown files\. Cognito indexes it on this computer\./,
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
          graphExtractionEnabled: true,
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
        <SourcesPanel />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Choose a folder" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("select_vault_folder");
      expect(invokeMock).toHaveBeenCalledWith("set_vault", { path: "/chosen" });
    });
  });

  it("shows graph waiting count for pending documents", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") {
        return [
          {
            id: "1",
            path: "/vault/a.pdf",
            title: "a",
            fileType: "pdf",
            contentHash: "abc",
            pageCount: 3,
            status: "indexed",
            errorMessage: null,
            importedAt: "2026-01-01T00:00:00Z",
            indexedAt: "2026-01-01T01:00:00Z",
            modifiedAt: "2026-01-01T01:00:00Z",
            graphStatus: "pending",
            graphError: null,
          },
          {
            id: "2",
            path: "/vault/b.md",
            title: "b",
            fileType: "markdown",
            contentHash: "def",
            pageCount: null,
            status: "indexed",
            errorMessage: null,
            importedAt: "2026-01-02T00:00:00Z",
            indexedAt: "2026-01-02T01:00:00Z",
            modifiedAt: "2026-01-02T01:00:00Z",
            graphStatus: "running",
            graphError: null,
          },
        ];
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
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Building graph, 2 files left")).toBeInTheDocument();
  });

  it("shows rejected import error with role alert", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") return [];
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          graphExtractionEnabled: true,
        };
      }
      if (cmd === "import_documents") {
        throw "No PDF or Markdown files found.";
      }
      return {};
    });
    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Import" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No PDF or Markdown files found.",
    );
  });
});
