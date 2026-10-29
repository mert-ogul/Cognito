import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ChatPanel,
  PreviewBody,
  citationsUsedInOrder,
  linkifyCitationMarkers,
} from "./ChatPanel";
import { RetrievalModeProvider } from "@/context/RetrievalModeContext";
import { listen } from "@/lib/tauri-events";
import { getDocumentPreview, openDocument, sendChatQuery } from "@/lib/tauri";
import type { CitationItem } from "@/lib/tauri";

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

const cite = (index: number, title = `Doc ${index}`): CitationItem => ({
  documentId: `d${index}`,
  title,
  path: `/p${index}`,
  pageStart: null,
  pageEnd: null,
  chunkId: `c${index}`,
  snippet: `snip ${index}`,
  nodeIds: [],
  index,
});

describe("ChatPanel helpers", () => {
  it("orders cited citations by first use", () => {
    const cites = [cite(1), cite(2), cite(3)];
    expect(citationsUsedInOrder("See [2] then [1] and [2] again", cites).map((c) => c.index)).toEqual([
      2, 1,
    ]);
  });

  it("linkifies citation markers outside code", () => {
    const out = linkifyCitationMarkers("Claim [2]. `code [2]` and\n```\n[2]\n```", new Set([2]));
    expect(out).toContain("[2](#cite-2)");
    expect(out).toContain("`code [2]`");
    expect(out).toContain("```\n[2]\n```");
  });
});

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

  it("citation button and cited chips", async () => {
    let chatStream:
      | ((ev: { payload: { messageId: string; token: string } }) => void)
      | undefined;
    let chatFinal:
      | ((ev: {
          payload: {
            messageId: string;
            citations: CitationItem[];
            retrievalMode: string;
            error?: string | null;
          };
        }) => void)
      | undefined;
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "chat-stream") chatStream = handler as typeof chatStream;
      if (event === "chat-final") chatFinal = handler as typeof chatFinal;
      return () => {};
    });
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      chatStream?.({ payload: { messageId, token: "Answer cites [2] only." } });
      await Promise.resolve();
      chatFinal?.({
        payload: {
          messageId,
          citations: [cite(1, "One"), cite(2, "Two")],
          retrievalMode: "hybrid",
          error: null,
        },
      });
      return messageId;
    });
    vi.mocked(getDocumentPreview).mockResolvedValue({
      documentId: "d2",
      text: "snip 2",
      highlightSnippet: "snip 2",
      pageStart: null,
      pageEnd: null,
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
    expect(await screen.findByLabelText("Open citation 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\[2\] Two/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /\[1\] One/ })).not.toBeInTheDocument();
    expect(screen.queryByText("Retrieved")).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Open citation 2"));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("d2", "c2");
    });
  });

  it("shows all citations under Retrieved when none are cited", async () => {
    let chatStream:
      | ((ev: { payload: { messageId: string; token: string } }) => void)
      | undefined;
    let chatFinal:
      | ((ev: {
          payload: {
            messageId: string;
            citations: CitationItem[];
            retrievalMode: string;
            error?: string | null;
          };
        }) => void)
      | undefined;
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "chat-stream") chatStream = handler as typeof chatStream;
      if (event === "chat-final") chatFinal = handler as typeof chatFinal;
      return () => {};
    });
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      chatStream?.({ payload: { messageId, token: "No numbers here." } });
      chatFinal?.({
        payload: {
          messageId,
          citations: [cite(1, "One"), cite(2, "Two")],
          retrievalMode: "vector_only",
          error: null,
        },
      });
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
    fireEvent.change(screen.getByLabelText("Chat message"), { target: { value: "ask" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText("Retrieved")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\[1\] One/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\[2\] Two/ })).toBeInTheDocument();
  });

  it("sends history that skips error messages", async () => {
    let chatFinal:
      | ((ev: {
          payload: {
            messageId: string;
            citations: CitationItem[];
            retrievalMode: string;
            error?: string | null;
          };
        }) => void)
      | undefined;
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "chat-final") chatFinal = handler as typeof chatFinal;
      return () => {};
    });
    vi.mocked(sendChatQuery)
      .mockRejectedValueOnce("boom")
      .mockImplementation(async (_q, messageId, history) => {
        expect(history?.some((t) => t.content === "boom")).toBeFalsy();
        expect(history?.some((t) => t.content === "first ok")).toBeTruthy();
        chatFinal?.({
          payload: {
            messageId,
            citations: [],
            retrievalMode: "vector_only",
            error: null,
          },
        });
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
    // First successful exchange
    vi.mocked(sendChatQuery).mockImplementationOnce(async (_q, messageId) => {
      chatFinal?.({
        payload: { messageId, citations: [], retrievalMode: "vector_only", error: null },
      });
      return messageId;
    });
    fireEvent.change(screen.getByLabelText("Chat message"), { target: { value: "first ok" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(sendChatQuery).toHaveBeenCalled());
    // Force an error on the assistant by mutating via reject
    vi.mocked(sendChatQuery).mockRejectedValueOnce("boom");
    fireEvent.change(screen.getByLabelText("Chat message"), { target: { value: "will fail" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText("boom")).toBeInTheDocument();
    // Third send should skip error content in history
    vi.mocked(sendChatQuery).mockImplementation(async (q, messageId, history) => {
      expect(q).toBe("again");
      expect(history?.every((t) => t.content !== "boom")).toBe(true);
      expect(history?.some((t) => t.content === "first ok")).toBe(true);
      chatFinal?.({
        payload: { messageId, citations: [], retrievalMode: "vector_only", error: null },
      });
      return messageId;
    });
    fireEvent.change(screen.getByLabelText("Chat message"), { target: { value: "again" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(sendChatQuery).toHaveBeenLastCalledWith("again", expect.any(String), expect.any(Array)));
  });

  it("highlights the whole cited chunk and Open file calls openDocument", async () => {
    let chatStream:
      | ((ev: { payload: { messageId: string; token: string } }) => void)
      | undefined;
    let chatFinal:
      | ((ev: {
          payload: {
            messageId: string;
            citations: CitationItem[];
            retrievalMode: string;
            error?: string | null;
          };
        }) => void)
      | undefined;
    vi.mocked(listen).mockImplementation(async (event, handler) => {
      if (event === "chat-stream") chatStream = handler as typeof chatStream;
      if (event === "chat-final") chatFinal = handler as typeof chatFinal;
      return () => {};
    });
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      chatStream?.({ payload: { messageId, token: "Hello [1]." } });
      chatFinal?.({
        payload: {
          messageId,
          citations: [
            {
              ...cite(1, "Report"),
              pageStart: 12,
              pageEnd: 12,
            },
          ],
          retrievalMode: "hybrid",
          error: null,
        },
      });
      return messageId;
    });
    vi.mocked(getDocumentPreview).mockResolvedValue({
      documentId: "d1",
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
    fireEvent.click(await screen.findByRole("button", { name: /\[1\] Report/ }));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("d1", "c1");
    });
    expect(await screen.findByText("Report · p. 12")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open file" }));
    await waitFor(() => {
      expect(openDocument).toHaveBeenCalledWith("d1");
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
