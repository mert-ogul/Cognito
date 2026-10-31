import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChatPanel, citationSourceLabel, normalizeAnswerMath } from "./ChatPanel";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import { SidePane } from "@/components/layout/SidePane";
import { e2eEmit } from "@/lib/tauri-events";
import {
  getDocumentPreview,
  getSuggestedQuestions,
  listDocuments,
  prepareModels,
  sendChatQuery,
} from "@/lib/tauri";
import type { CitationItem, DocumentRow } from "@/lib/tauri";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    sendChatQuery: vi.fn(),
    warmChatModel: vi.fn(async () => {}),
    prepareModels: vi.fn(async () => ({ ready: true, models: [], totalSizeBytes: 0 })),
    getModelsStatus: vi.fn(async () => ({ ready: true, models: [], totalSizeBytes: 0 })),
    getDocumentPreview: vi.fn(async (documentId: string) => ({
      documentId,
      text: null,
      highlightSnippet: null,
      pageStart: null,
      pageEnd: null,
      regions: null,
    })),
    renderPageRegion: vi.fn(async () => ({
      pngBase64: actual.TINY_PNG_BASE64,
      width: 1,
      height: 1,
    })),
    renderDocumentPage: vi.fn(async () => ({
      pngBase64: actual.TINY_PNG_BASE64,
      width: 1,
      height: 1,
    })),
    openDocument: vi.fn(async () => {}),
    getSuggestedQuestions: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({
      vaultPath: "/vault",
      chatModel: "qwen3.5:9b",
      embedModel: "qwen3-embedding:0.6b",
      backgroundModel: "qwen3.5:9b",
      summariseDocuments: true,
      extractEntities: false,
      extraEntityLabels: "",
    })),
    listDocuments: vi.fn(async () => []),
  };
});

const health = {
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
  missingModels: [] as string[],
  indexFormatUpdating: false,
  checkingModelsReady: true,
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

const doc = (id: string, title: string): DocumentRow => ({
  id,
  path: `/vault/${title}.md`,
  title,
  fileType: "markdown",
  contentHash: id,
  pageCount: null,
  status: "indexed",
  errorMessage: null,
  importedAt: "2026-01-01T00:00:00Z",
  indexedAt: "2026-01-01T01:00:00Z",
  modifiedAt: "2026-01-01T00:30:00Z",
  entitiesStatus: "ok",
  entitiesError: null,
  cardStatus: "ok",
  cardType: "note",
  cardDate: null,
  cardPeople: [],
  cardOrganizations: [],
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
    vi.mocked(listDocuments).mockReset();
    vi.mocked(prepareModels).mockReset();
    vi.mocked(getSuggestedQuestions).mockReset();
    vi.mocked(listDocuments).mockResolvedValue([]);
    vi.mocked(prepareModels).mockResolvedValue({ ready: true, models: [], totalSizeBytes: 0 });
    vi.mocked(getSuggestedQuestions).mockResolvedValue([]);
    useSidePaneStore.setState({
      open: false,
      mode: "connections",
      passage: null,
      highlightNodeIds: [],
    });
    useChatIntentStore.setState({
      pendingScope: [],
      pendingQuestion: null,
      pendingRelated: null,
    });
  });

  it("renders answer sentences in one paragraph with unsupported titled", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1)],
        sentences: [
          { text: "Supported claim.", status: "supported", sources: [1] },
          { text: "Guessed claim.", status: "unsupported", sources: [] },
        ],
        notFound: false,
        error: null,
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("ask");
    const guessed = await screen.findByText("Guessed claim.");
    expect(guessed).toBeInTheDocument();
    expect(guessed).toHaveAttribute("title", "Not found in the sources");
    expect(screen.queryByText("Not found in the sources")).toBeNull();
    expect(screen.getByText("Supported claim.").closest("div.answer")).toBe(
      guessed.closest("div.answer"),
    );
  });

  it("renders one unchecked note under the paragraph", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1)],
        sentences: [
          { text: "Timed out claim.", status: "unchecked", sources: [1] },
        ],
        notFound: false,
        error: null,
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("ask");
    expect(await screen.findByText("Timed out claim.")).toBeInTheDocument();
    expect(screen.getByText("Some sentences couldn't be checked.")).toBeInTheDocument();
    expect(screen.queryByText("Not checked")).toBeNull();
  });

  it("renders a formula note when any sentence reason is formula", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1)],
        sentences: [
          {
            text: "$$H = AW$$",
            status: "unchecked",
            sources: [1],
            reason: "formula",
          },
        ],
        notFound: false,
        error: null,
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("ask");
    expect(
      await screen.findByText(
        "Formulas are shown as written by the model; compare them with the page images below.",
      ),
    ).toBeInTheDocument();
  });

  it("adds and removes @ scope documents", async () => {
    vi.mocked(listDocuments).mockResolvedValue([
      doc("a", "Alpha Notes"),
      doc("b", "Beta Paper"),
    ]);
    renderChat();
    const input = screen.getByLabelText("Chat message");
    expect(screen.getByText("Searching all documents")).toBeInTheDocument();
    fireEvent.change(input, { target: { value: "@Alp" } });
    expect(await screen.findByRole("option", { name: "Alpha Notes" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Alpha Notes" }));
    expect(screen.getByText(/In: Alpha Notes/)).toBeInTheDocument();
    expect(input).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Remove Alpha Notes from scope" }));
    expect(screen.getByText("Searching all documents")).toBeInTheDocument();
  });

  it("shows passages before the answer", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-stage", {
        messageId,
        stage: "passages",
        citations: [cite(1, "One", "/vault/one.pdf")],
      });
      await new Promise((r) => setTimeout(r, 20));
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1, "One", "/vault/one.pdf")],
        sentences: [{ text: "Final answer body.", status: "supported", sources: [1] }],
        notFound: false,
        error: null,
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("ask");
    expect(await screen.findByText("Sources")).toBeInTheDocument();
    expect(screen.getByText("Writing the answer")).toBeInTheDocument();
    expect(await screen.findByText("Final answer body.")).toBeInTheDocument();
  });

  it("shows notFound passages with title and page separate from snippet", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [
          {
            ...cite(1, "Closest Paper", "/vault/c.pdf"),
            pageStart: 3,
            pageEnd: 3,
            snippet:
              "Intro fluff. The classification layer uses equation two for training. More padding after that sentence follows here for length.",
          },
        ],
        sentences: [],
        notFound: true,
        error: null,
        suggestions: [],
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("classification equation");
    expect(
      await screen.findByText("Your documents don't answer this."),
    ).toBeInTheDocument();
    expect(screen.getByText("Closest Paper")).toBeInTheDocument();
    expect(screen.getByText("p. 3")).toBeInTheDocument();
    const btn = screen.getByRole("button", { name: /Closest Paper/ });
    expect(btn.textContent).toMatch(/Closest Paper/);
    expect(btn.textContent).toMatch(/p\. 3/);
    expect(btn.textContent).toMatch(/classification layer/);
    expect(btn.textContent).not.toMatch(/Closest Paperp\./);
  });

  it("shows unconfirmed answer disclosure when notFound has sentences", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [cite(1, "Closest Paper", "/vault/c.pdf")],
        sentences: [
          { text: "Drafted but unconfirmed.", status: "unsupported", sources: [] },
        ],
        notFound: true,
        error: null,
        suggestions: [],
      });
      return messageId;
    });
    renderChat();
    await sendPrompt("mars capital");
    expect(
      await screen.findByText("Your documents don't answer this."),
    ).toBeInTheDocument();
    const disclosure = screen.getByText("Show the unconfirmed answer").closest("details");
    expect(disclosure).not.toBeNull();
    expect(disclosure).not.toHaveAttribute("open");
    expect(disclosure?.textContent).toMatch(/Drafted but unconfirmed/);
    fireEvent.click(screen.getByText("Show the unconfirmed answer"));
    expect(disclosure).toHaveAttribute("open");
  });

  it("suggestion link sets scope and resends the question", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId, _h, scopeIds) => {
      if (!scopeIds?.length) {
        e2eEmit("chat-final", {
          messageId,
          citations: [cite(1, "GAT", "/vault/gat.pdf")],
          sentences: [],
          notFound: true,
          error: null,
          suggestions: [
            { documentId: "gcn", title: "GCN Paper", label: "Search in GCN Paper" },
          ],
        });
      } else {
        e2eEmit("chat-final", {
          messageId,
          citations: [cite(1, "GCN Paper", "/vault/gcn.pdf")],
          sentences: [
            { text: "Equation (2) is the layer rule.", status: "supported", sources: [1] },
          ],
          notFound: false,
          error: null,
          scopeTitle: "GCN Paper",
        });
      }
      return messageId;
    });
    vi.mocked(listDocuments).mockResolvedValue([
      doc("gcn", "GCN Paper"),
      doc("gat", "GAT"),
    ]);
    renderChat();
    await sendPrompt("Write Equation (2) from Kipf");
    const link = await screen.findByRole("button", { name: "Search in GCN Paper" });
    fireEvent.click(link);
    await waitFor(() => {
      expect(sendChatQuery).toHaveBeenCalledTimes(2);
    });
    const second = vi.mocked(sendChatQuery).mock.calls[1];
    expect(second[0]).toBe("Write Equation (2) from Kipf");
    expect(second[3]).toEqual(["gcn"]);
  });

  it("shows models download gate when checking models are missing", () => {
    renderChat(
      <ChatPanel
        health={{
          ...health,
          checkingModelsReady: false,
        }}
      />,
    );
    expect(
      screen.getByText(
        "Cognito needs checking models, downloaded once.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
    expect(screen.getByLabelText("Chat message")).toBeDisabled();
  });

  it("downloads checking models when Download is clicked", async () => {
    renderChat(
      <ChatPanel
        health={{
          ...health,
          checkingModelsReady: false,
        }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(prepareModels).toHaveBeenCalled());
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
    expect(screen.getByPlaceholderText("Loading qwen3.5:9b")).toBeInTheDocument();
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
        sentences: [],
        notFound: false,
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

  it("superscript citation opens Passage mode with the cited chunk marked", async () => {
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [
          cite(1, "One", "/vault/one.pdf"),
          { ...cite(2, "Two", "/vault/two.pdf"), pageStart: 3, pageEnd: 3 },
        ],
        sentences: [{ text: "Answer cites only this.", status: "supported", sources: [2] }],
        notFound: false,
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
    fireEvent.click(screen.getByLabelText("Source 2"));
    await waitFor(() => {
      expect(getDocumentPreview).toHaveBeenCalledWith("d2", "c2");
    });
    expect(useSidePaneStore.getState().open).toBe(true);
    expect(useSidePaneStore.getState().mode).toBe("passage");
    expect(screen.getByRole("button", { name: "Passage", pressed: true })).toBeInTheDocument();
    const mark = await screen.findByText("cited body");
    expect(mark.className).toMatch(/bg-mark/);
  });

  it("sends scope ids with the query", async () => {
    vi.mocked(listDocuments).mockResolvedValue([doc("a", "Alpha Notes")]);
    vi.mocked(sendChatQuery).mockImplementation(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [],
        sentences: [{ text: "ok", status: "supported", sources: [] }],
        notFound: false,
        error: null,
      });
      return messageId;
    });
    renderChat();
    const input = screen.getByLabelText("Chat message");
    fireEvent.change(input, { target: { value: "@" } });
    fireEvent.click(await screen.findByRole("option", { name: "Alpha Notes" }));
    await sendPrompt("scoped ask");
    await waitFor(() =>
      expect(sendChatQuery).toHaveBeenCalledWith(
        "scoped ask",
        expect.any(String),
        expect.any(Array),
        ["a"],
      ),
    );
  });

  it("sends history that skips error messages", async () => {
    renderChat();
    vi.mocked(sendChatQuery).mockImplementationOnce(async (_q, messageId) => {
      e2eEmit("chat-final", {
        messageId,
        citations: [],
        sentences: [{ text: "ok reply", status: "supported", sources: [] }],
        notFound: false,
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
        sentences: [{ text: "again ok", status: "supported", sources: [] }],
        notFound: false,
        error: null,
      });
      return messageId;
    });
    await sendPrompt("again");
    await waitFor(() =>
      expect(sendChatQuery).toHaveBeenLastCalledWith(
        "again",
        expect.any(String),
        expect.any(Array),
        [],
      ),
    );
  });

  it("suggested question sets input and scope", async () => {
    vi.mocked(getSuggestedQuestions).mockResolvedValue([
      { question: "What does Ada do?", scopeIds: ["a"], label: null },
    ]);
    vi.mocked(listDocuments).mockResolvedValue([doc("a", "Alpha Notes"), doc("b", "Beta Paper")]);
    renderChat();
    fireEvent.click(await screen.findByRole("button", { name: "What does Ada do?" }));
    await waitFor(() => {
      expect(screen.getByLabelText("Chat message")).toHaveValue("What does Ada do?");
    });
    expect(screen.getByText(/In: Alpha Notes/)).toBeInTheDocument();
  });
});
