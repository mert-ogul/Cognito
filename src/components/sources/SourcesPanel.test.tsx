import { listen } from "@/lib/tauri-events";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SourcesPanel } from "./SourcesPanel";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@/lib/tauri-events", () => ({
  listen: vi.fn(),
}));

describe("SourcesPanel", () => {
  beforeEach(() => {
    vi.mocked(listen).mockImplementation(async () => () => {});
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
          telemetryOptIn: false,
          crashReportingOptIn: false,
          graphExtractionEnabled: true,
        };
      }
      return {};
    });
  });

  it("renders document statuses, PDF page count, and error badge", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("a.pdf")).toBeInTheDocument();
    expect(screen.getByText("indexed")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("failed")).toBeInTheDocument();
    expect(screen.getByText("Error")).toBeInTheDocument();
  });

  it("shows import progress when import-progress event fires", async () => {
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "import-progress") {
        queueMicrotask(() => {
          handler({
            payload: {
              jobId: "j1",
              status: "running",
              totalFiles: 10,
              completedFiles: 4,
              failedFiles: 0,
              phase: "parse",
              path: "/vault/x.pdf",
            },
          });
        });
      }
      return () => {};
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <SourcesPanel />
      </QueryClientProvider>,
    );
    expect(await screen.findByText(/Import 4\/10/)).toBeInTheDocument();
    expect(screen.getByText(/parse/)).toBeInTheDocument();
  });

  it("shows empty-state guidance when there are no documents", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_documents") return [];
      if (cmd === "get_settings") {
        return {
          vaultPath: null,
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          telemetryOptIn: false,
          crashReportingOptIn: false,
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
    expect(await screen.findByText("Local library")).toBeInTheDocument();
    expect(screen.getByText(/100 documents/)).toBeInTheDocument();
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
          telemetryOptIn: false,
          crashReportingOptIn: false,
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
    expect(await screen.findByText("Graph: 2 documents waiting")).toBeInTheDocument();
  });
});
