import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChatPanel, PreviewBody } from "./ChatPanel";
import { RetrievalModeProvider } from "@/context/RetrievalModeContext";
import { listen } from "@/lib/tauri-events";
import { getDocumentPreview, openDocument, sendChatQuery } from "@/lib/tauri";

vi.mock("@/lib/tauri-events", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    sendChatQuery: vi.fn(),
    warmChatModel: vi.fn(async () => {}),
    getDocumentPreview: vi.fn(),
    openDocument: vi.fn(async () => {}),
  };
});

const health = {
  sqliteOk: true,
  duckdbOk: true,
  sidecarReachable: true,
  ollamaModelReady: true,
  vaultConfigured: true,
  chatModel: "phi4-mini",
  embedModel: "nomic-embed-text",
  chatModelWarmed: true,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: true,
  missingModels: [] as string[],
};

describe("ChatPanel", () => {
  beforeEach(() => {
    vi.mocked(listen).mockImplementation(async () => () => {});
    vi.mocked(sendChatQuery).mockReset();
    vi.mocked(getDocumentPreview).mockReset();
    vi.mocked(openDocument).mockReset();
  });

  it("disables send when model not warmed", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <RetrievalModeProvider>
          <ChatPanel
            health={{
              ...health,
              chatModelWarmed: false,
              warmUpPending: true,
            }}
          />
        </RetrievalModeProvider>
      </QueryClientProvider>,
    );
    expect(screen.getByPlaceholderText(/Loading model/)).toBeInTheDocument();
  });

  it("shows error and re-enables input when sendChatQuery rejects", async () => {
    vi.mocked(sendChatQuery).mockRejectedValue("sidecar down");
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <RetrievalModeProvider>
          <ChatPanel health={health} />
        </RetrievalModeProvider>
      </QueryClientProvider>,
    );
    const input = screen.getByLabelText("Chat message");
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText("sidecar down")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText("Chat message")).not.toBeDisabled();
    });
  });

  it("clears busy when chat-final arrives before sendChatQuery resolves", async () => {
    let chatFinal:
      | ((ev: {
          payload: {
            messageId: string;
            citations: [];
            retrievalMode: string;
            error?: string | null;
          };
        }) => void)
      | undefined;
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "chat-final") {
        chatFinal = handler as typeof chatFinal;
      }
      return () => {};
    });
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      chatFinal?.({
        payload: {
          messageId,
          citations: [],
          retrievalMode: "error",
          error: "fast fail",
        },
      });
      await new Promise((r) => setTimeout(r, 20));
      return messageId;
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <RetrievalModeProvider>
          <ChatPanel health={health} />
        </RetrievalModeProvider>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(chatFinal).toBeDefined());
    const input = screen.getByLabelText("Chat message");
    fireEvent.change(input, { target: { value: "race" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText("fast fail")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText("Chat message")).not.toBeDisabled();
    });
  });

  it("highlights the whole cited chunk and Open file calls openDocument", async () => {
    let chatFinal:
      | ((ev: {
          payload: {
            messageId: string;
            citations: Array<{
              documentId: string;
              title: string;
              path: string;
              pageStart: number | null;
              pageEnd: number | null;
              chunkId: string;
              snippet: string;
              nodeIds: string[];
            }>;
            retrievalMode: string;
            error?: string | null;
          };
        }) => void)
      | undefined;
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "chat-final") {
        chatFinal = handler as typeof chatFinal;
      }
      return () => {};
    });
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      chatFinal?.({
        payload: {
          messageId,
          citations: [
            {
              documentId: "doc-1",
              title: "Report",
              path: "/vault/report.pdf",
              pageStart: 12,
              pageEnd: 12,
              chunkId: "chunk-1",
              snippet: "cited body",
              nodeIds: [],
            },
          ],
          retrievalMode: "hybrid",
          error: null,
        },
      });
      return messageId;
    });
    vi.mocked(getDocumentPreview).mockResolvedValue({
      documentId: "doc-1",
      text: "prev\n\ncited body\n\nnext",
      highlightSnippet: "cited body",
      pageStart: 12,
      pageEnd: 12,
    });
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <RetrievalModeProvider>
          <ChatPanel health={health} />
        </RetrievalModeProvider>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(chatFinal).toBeDefined());
    fireEvent.change(screen.getByLabelText("Chat message"), { target: { value: "ask" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    fireEvent.click(await screen.findByRole("button", { name: /Report/ }));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("doc-1", "chunk-1");
    });
    expect(await screen.findByText("Report · p. 12")).toBeInTheDocument();
    const mark = await screen.findByText("cited body");
    expect(mark.tagName.toLowerCase()).toBe("mark");
    fireEvent.click(screen.getByRole("button", { name: "Open file" }));
    await waitFor(() => {
      expect(openDocument).toHaveBeenCalledWith("doc-1");
    });
  });
});

describe("PreviewBody", () => {
  it("marks the exact highlight substring", () => {
    render(<PreviewBody text={"aaa\n\nwhole cited chunk\n\nbbb"} highlight="whole cited chunk" />);
    const mark = screen.getByText("whole cited chunk");
    expect(mark.tagName.toLowerCase()).toBe("mark");
  });
});
