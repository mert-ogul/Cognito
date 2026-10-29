import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsSheet } from "./SettingsSheet";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.1.0"),
}));

describe("SettingsSheet", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_settings") {
        return {
          vaultPath: "/vault",
          chatModel: "phi4-mini",
          embedModel: "nomic-embed-text",
          graphExtractionEnabled: true,
        };
      }
      if (cmd === "list_documents") {
        return [
          {
            id: "1",
            path: "/vault/a.md",
            title: "a",
            fileType: "markdown",
            contentHash: "h",
            pageCount: null,
            status: "indexed",
            errorMessage: null,
            importedAt: "2026-01-01T00:00:00Z",
            indexedAt: "2026-01-01T01:00:00Z",
            modifiedAt: "2026-01-01T01:00:00Z",
            graphStatus: "ok",
            graphError: null,
          },
        ];
      }
      if (cmd === "select_vault_folder") {
        return "/vault-b";
      }
      return {};
    });
  });

  it("shows rebuild confirmation when embedding model changes, cancel restores old value", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <SettingsSheet open onOpenChange={() => {}} />
      </QueryClientProvider>,
    );
    const input = await screen.findByLabelText("Embedding model");
    await waitFor(() => expect(input).toHaveValue("nomic-embed-text"));
    fireEvent.change(input, { target: { value: "mxbai-embed-large" } });
    expect(await screen.findByRole("button", { name: "Save and rebuild index" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save and rebuild index" }));
    expect(
      await screen.findByText(
        /All 1 documents will be indexed again with mxbai-embed-large, and the graph will be rebuilt./,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.getByLabelText("Embedding model")).toHaveValue("nomic-embed-text"));
  });

  it("shows Change folder confirmation with document count; Cancel leaves settings unchanged", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <SettingsSheet open onOpenChange={() => {}} />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("/vault")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
    expect(
      await screen.findByText(
        /Switching to \/vault-b removes the 1 documents from \/vault from Cognito\. Your files stay where they are\./,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByText(/Switching to/)).not.toBeInTheDocument();
    });
    expect(invokeMock).not.toHaveBeenCalledWith("set_vault", expect.anything());
    expect(screen.getByText("/vault")).toBeInTheDocument();
  });
});
