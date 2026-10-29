import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChatPanel } from "./ChatPanel";
import { RetrievalModeProvider } from "@/context/RetrievalModeContext";
import { listen } from "@/lib/tauri-events";
import { sendChatQuery } from "@/lib/tauri";

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
});
