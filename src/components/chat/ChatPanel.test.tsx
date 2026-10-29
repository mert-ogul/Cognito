import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ChatPanel,
  citationSourceLabel,
  citationsUsedInOrder,
  linkifyCitationMarkers,
  normalizeAnswerMath,
} from "./ChatPanel";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import { SidePane } from "@/components/layout/SidePane";
import { e2eEmit } from "@/lib/tauri-events";
import { getDocumentPreview, openDocument, sendChatQuery } from "@/lib/tauri";
import type { CitationItem } from "@/lib/tauri";
import { useSidePaneStore } from "@/stores/sidePaneStore";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    sendChatQuery: vi.fn(),
    warmChatModel: vi.fn(async () => {}),
    getDocumentPreview: vi.fn(),
    openDocument: vi.fn(async () => {}),
    getGraphSnapshot: vi.fn(async () => ({ nodes: [], edges: [] })),
    searchGraphNodes: vi.fn(async () => []),
    getGraphNodeDetail: vi.fn(async () => null),
    getSettings: vi.fn(async () => ({
      vaultPath: "/vault",
      chatModel: "phi4-mini",
      embedModel: "nomic-embed-text",
      graphExtractionEnabled: true,
    })),
    listDocuments: vi.fn(async () => []),
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
  indexFormatUpdating: false,
};

const cite = (index: number, title = `Doc ${index}`, path = `/p${index}/${title}.pdf`): CitationItem => ({
  documentId: `d${index}`,
  title,
  path,
  pageStart: null,
  pageEnd: null,
  chunkId: `c${index}`,
  snippet: `snip ${index}`,
  nodeIds: index === 2 ? ["n-a", "n-b"] : [],
  index,
});

function renderChat(ui: React.ReactNode = <ChatPanel health={health} />) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

async function sendPrompt(text: string) {
  const input = screen.getByLabelText("Chat message");
  fireEvent.change(input, { target: { value: text } });
  fireEvent.keyDown(input, { key: "Enter", code: "Enter", shiftKey: false });
}

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

  it("formats source labels with pages", () => {
    expect(
      citationSourceLabel({
        ...cite(1, "Report", "/vault/report.pdf"),
        pageStart: 12,
        pageEnd: 12,
      }),
    ).toBe("Report, p. 12");
  });

  it("normalizes paren math delimiters outside code", () => {
    expect(normalizeAnswerMath(String.raw`inline \(x^2\) and`)).toBe("inline $x^2$ and");
    expect(normalizeAnswerMath(String.raw`$$\frac{1}{2}$$`)).toBe(String.raw`$$\frac{1}{2}$$`);
    expect(normalizeAnswerMath(String.raw`\[a\]`)).toBe("$$a$$");
    expect(normalizeAnswerMath("keep `\\(x\\)`")).toBe("keep `\\(x\\)`");
  });
});

function renderAnswerMath(md: string) {
  return render(
    <div className="answer">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[[rehypeKatex, { throwOnError: false }]]}
      >
        {normalizeAnswerMath(md)}
      </ReactMarkdown>
    </div>,
  );
}

describe("answer math", () => {
  it("renders $ and \\( \\) as katex", () => {
    const { container: a } = renderAnswerMath("$x^2$");
    expect(a.querySelector(".katex")).not.toBeNull();
    const { container: b } = renderAnswerMath(String.raw`\(x^2\)`);
    expect(b.querySelector(".katex")).not.toBeNull();
  });

  it("does not throw on unfinished display math", () => {
    expect(() => renderAnswerMath(String.raw`$$\frac{1`)).not.toThrow();
  });
});

describe("ChatPanel", () => {
  beforeEach(() => {
    vi.mocked(sendChatQuery).mockReset();
    vi.mocked(getDocumentPreview).mockReset();
    vi.mocked(openDocument).mockReset();
    useSidePaneStore.setState({
      open: false,
      mode: "graph",
      passage: null,
      highlightNodeIds: [],
    });
  });

  it("disables send when model not warmed", () => {
    renderChat(
      <ChatPanel
        health={{
          ...health,
          chatModelWarmed: false,
          warmUpPending: true,
        }}
      />,
    );
    expect(screen.getByPlaceholderText("Loading phi4-mini")).toBeInTheDocument();
  });

  it("shows error and re-enables input when sendChatQuery rejects", async () => {
    vi.mocked(sendChatQuery).mockRejectedValue("sidecar down");
    renderChat();
    await sendPrompt("hello");
    expect(await screen.findByText("sidecar down")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText("Chat message")).not.toBeDisabled();
    });
  });

  it("clears busy when chat-final arrives before sendChatQuery resolves", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [],
        retrievalMode: "error",
        error: "fast fail",
      });
      await new Promise((r) => setTimeout(r, 20));
      return messageId;
    });
    renderChat();
    await sendPrompt("race");
    expect(await screen.findByText("fast fail")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText("Chat message")).not.toBeDisabled();
    });
  });

  it("under StrictMode each streamed token appears once", async () => {
    let assistantId = "";
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      assistantId = messageId;
      return messageId;
    });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <StrictMode>
        <QueryClientProvider client={client}>
          <ChatPanel health={health} />
        </QueryClientProvider>
      </StrictMode>,
    );
    await sendPrompt("stream me");
    await waitFor(() => expect(assistantId).toBeTruthy());
    act(() => {
      e2eEmit("chat-stream", { messageId: assistantId, token: "One" });
      e2eEmit("chat-stream", { messageId: assistantId, token: " interesting" });
    });
    expect(await screen.findByText("One interesting")).toBeInTheDocument();
    expect(screen.queryByText("OneOne interesting interesting")).not.toBeInTheDocument();
  });

  it("superscript citation opens Passage mode with the cited chunk marked", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-stream", { messageId, token: "Answer cites [2] only." });
      await Promise.resolve();
      e2eEmit("chat-final", {
        messageId,
        citations: [
          cite(1, "One", "/vault/one.pdf"),
          { ...cite(2, "Two", "/vault/two.pdf"), pageStart: 3, pageEnd: 3 },
        ],
        retrievalMode: "hybrid",
        error: null,
      });
      return messageId;
    });
    vi.mocked(getDocumentPreview).mockResolvedValue({
      documentId: "d2",
      text: "prev\n\ncited body\n\nnext",
      highlightSnippet: "cited body",
      pageStart: 3,
      pageEnd: 3,
    });
    renderChat(
      <>
        <ChatPanel health={health} />
        <SidePane />
      </>,
    );
    await sendPrompt("ask");
    expect(await screen.findByLabelText("Source 2")).toBeInTheDocument();
    expect(screen.getByText("Sources")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Two, p. 3" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^One$/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Source 2"));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("d2", "c2");
    });
    expect(useSidePaneStore.getState().open).toBe(true);
    expect(useSidePaneStore.getState().mode).toBe("passage");
    expect(screen.getByRole("button", { name: "Passage", pressed: true })).toBeInTheDocument();
    expect(screen.getAllByText("Two, p. 3").length).toBeGreaterThanOrEqual(2);
    const mark = await screen.findByText("cited body");
    expect(mark.className).toMatch(/bg-mark/);
  });

  it("lists all passages under Retrieved passages when none are cited", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-stream", { messageId, token: "No numbers here." });
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1, "One", "/vault/one.pdf"), cite(2, "Two", "/vault/two.pdf")],
        retrievalMode: "vector_only",
        error: null,
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("ask");
    expect(await screen.findByText("Retrieved passages")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "One" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Two" })).toBeInTheDocument();
  });

  it("renders Markdown lists as ul and li", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-stream", {
        messageId,
        token: "Items:\n\n- alpha\n- beta",
      });
      e2eEmit("chat-final", {
        messageId,
        citations: [],
        retrievalMode: "vector_only",
        error: null,
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("list");
    const answer = await screen.findByText("alpha");
    const list = answer.closest("ul");
    expect(list).not.toBeNull();
    expect(within(list!).getByText("beta").tagName.toLowerCase()).toBe("li");
  });

  it("Show related entities switches the pane to Graph", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-stream", { messageId, token: "Uses graph [2]." });
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1), cite(2, "Two", "/vault/two.pdf")],
        retrievalMode: "hybrid",
        error: null,
      });
      return messageId;
    });
    renderChat(
      <>
        <ChatPanel health={health} />
        <SidePane />
      </>,
    );
    await sendPrompt("ask");
    fireEvent.click(await screen.findByRole("button", { name: "Show related entities in the graph" }));
    expect(useSidePaneStore.getState().open).toBe(true);
    expect(useSidePaneStore.getState().mode).toBe("graph");
    expect(useSidePaneStore.getState().highlightNodeIds).toEqual(["n-a", "n-b"]);
    expect(screen.getByRole("button", { name: "Graph", pressed: true })).toBeInTheDocument();
  });

  it("sends history that skips error messages", async () => {
    renderChat();
    vi.mocked(sendChatQuery).mockImplementationOnce(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [],
        retrievalMode: "vector_only",
        error: null,
      });
      return messageId;
    });
    await sendPrompt("first ok");
    await waitFor(() => expect(sendChatQuery).toHaveBeenCalled());
    vi.mocked(sendChatQuery).mockRejectedValueOnce("boom");
    await sendPrompt("will fail");
    expect(await screen.findByText("boom")).toBeInTheDocument();
    vi.mocked(sendChatQuery).mockImplementation(async (q, messageId, history) => {
      expect(q).toBe("again");
      expect(history?.every((t) => t.content !== "boom")).toBe(true);
      expect(history?.some((t) => t.content === "first ok")).toBe(true);
      e2eEmit("chat-final", {
        messageId,
        citations: [],
        retrievalMode: "vector_only",
        error: null,
      });
      return messageId;
    });
    await sendPrompt("again");
    await waitFor(() =>
      expect(sendChatQuery).toHaveBeenLastCalledWith("again", expect.any(String), expect.any(Array)),
    );
  });
});
