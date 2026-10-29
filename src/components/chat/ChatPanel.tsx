import { Button } from "@/components/ui/button";
import { useRetrievalMode } from "@/context/RetrievalModeContext";
import {
  getDocumentPreview,
  openDocument,
  sendChatQuery,
  warmChatModel,
  type AppHealth,
  type ChatTurn,
  type CitationItem,
} from "@/lib/tauri";
import { listen } from "@/lib/tauri-events";
import { useGraphSessionStore } from "@/stores/graphSessionStore";
import { cn } from "@/lib/utils";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

type ChatMessage = {
  id: string;
  role: "user" | "assistant";
  text: string;
  citations?: CitationItem[];
  retrievalMode?: string;
  error?: string;
};

type Props = {
  health: AppHealth | undefined;
};

export function citationsUsedInOrder(text: string, citations: CitationItem[]): CitationItem[] {
  const byIndex = new Map(citations.map((c) => [c.index, c]));
  const seen = new Set<number>();
  const out: CitationItem[] = [];
  for (const m of text.matchAll(/\[(\d+)\]/g)) {
    const n = Number(m[1]);
    const c = byIndex.get(n);
    if (c && !seen.has(n)) {
      seen.add(n);
      out.push(c);
    }
  }
  return out;
}

/** Turn [n] into markdown links outside code spans/blocks. */
export function linkifyCitationMarkers(text: string, indexes: Set<number>): string {
  const parts = text.split(/(```[\s\S]*?```|`[^`\n]+`)/g);
  return parts
    .map((part) => {
      if (part.startsWith("`")) return part;
      return part.replace(/\[(\d+)\]/g, (full, nStr: string) => {
        const n = Number(nStr);
        if (!indexes.has(n)) return full;
        return `[${n}](#cite-${n})`;
      });
    })
    .join("");
}

export function ChatPanel({ health }: Props) {
  const queryClient = useQueryClient();
  const { setLastRetrievalMode } = useRetrievalMode();
  const setHighlightNodeIds = useGraphSessionStore((s) => s.setHighlightNodeIds);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [retryUserText, setRetryUserText] = useState<string | null>(null);
  const lastSentRef = useRef<string | null>(null);
  const warmRetryErrorRef = useRef<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const [preview, setPreview] = useState<{
    documentId: string;
    text: string | null;
    highlight: string | null;
    title: string;
  } | null>(null);

  const ready =
    !!health?.sidecarReachable &&
    !!health?.chatModelWarmed &&
    !health?.warmUpPending;

  useEffect(() => {
    const el = bottomRef.current;
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ behavior: "smooth" });
    }
  }, [messages, busyId]);

  const retryWarmUp = useCallback(async () => {
    await warmChatModel();
    await queryClient.invalidateQueries({ queryKey: ["health"] });
  }, [queryClient]);

  useEffect(() => {
    if (!health?.ollamaModelReady || !health.warmUpError) {
      return;
    }
    if (warmRetryErrorRef.current === health.warmUpError) {
      return;
    }
    warmRetryErrorRef.current = health.warmUpError;
    void retryWarmUp();
  }, [health?.ollamaModelReady, health?.warmUpError, retryWarmUp]);

  useEffect(() => {
    const un: Array<() => void> = [];
    let cancelled = false;
    void (async () => {
      const u1 = await listen<{ messageId: string; token: string }>("chat-stream", (ev) => {
        const { messageId, token } = ev.payload;
        setMessages((prev) =>
          prev.map((m) =>
            m.id === messageId && m.role === "assistant" ? { ...m, text: m.text + token } : m,
          ),
        );
      });
      const u2 = await listen<{
        messageId: string;
        citations: CitationItem[];
        retrievalMode: string;
        error?: string | null;
      }>("chat-final", (ev) => {
        const p = ev.payload;
        setLastRetrievalMode(p.retrievalMode ?? null);
        setBusyId(null);
        if (p.error) {
          setRetryUserText(lastSentRef.current);
        } else {
          setRetryUserText(null);
        }
        setMessages((prev) =>
          prev.map((m) =>
            m.id === p.messageId && m.role === "assistant"
              ? {
                  ...m,
                  citations: p.citations,
                  retrievalMode: p.retrievalMode,
                  error: p.error ?? undefined,
                }
              : m,
          ),
        );
      });
      if (!cancelled) {
        un.push(u1, u2);
      }
    })();
    return () => {
      cancelled = true;
      un.forEach((f) => f());
    };
  }, [setLastRetrievalMode]);

  const openCitation = useCallback(
    async (c: CitationItem) => {
      setHighlightNodeIds(c.nodeIds ?? []);
      const prev = await getDocumentPreview(c.documentId, c.chunkId);
      const pageStart = prev.pageStart ?? c.pageStart;
      const pageEnd = prev.pageEnd ?? c.pageEnd;
      let pageLabel = "";
      if (pageStart != null && pageEnd != null && pageEnd !== pageStart) {
        pageLabel = ` · pp. ${pageStart}-${pageEnd}`;
      } else if (pageStart != null) {
        pageLabel = ` · p. ${pageStart}`;
      }
      setPreview({
        documentId: prev.documentId,
        text: prev.text,
        highlight: prev.highlightSnippet ?? c.snippet,
        title: `${c.title}${pageLabel}`,
      });
    },
    [setHighlightNodeIds],
  );

  const sendWithText = useCallback(
    async (q: string) => {
      const query = q.trim();
      if (!query || !ready || busyId) return;
      setRetryUserText(null);
      lastSentRef.current = query;
      const history: ChatTurn[] = messages
        .filter((m) => !m.error && m.text.trim().length > 0)
        .slice(-6)
        .map((m) => ({ role: m.role, content: m.text }));
      const userId = crypto.randomUUID();
      const assistantId = crypto.randomUUID();
      setMessages((prev) => [
        ...prev,
        { id: userId, role: "user", text: query },
        { id: assistantId, role: "assistant", text: "" },
      ]);
      setBusyId(assistantId);
      try {
        await sendChatQuery(query, assistantId, history);
      } catch (e) {
        const msg = String(e);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant" ? { ...m, error: msg } : m,
          ),
        );
        setBusyId(null);
        setRetryUserText(query);
      }
    },
    [ready, busyId, messages],
  );

  const send = useCallback(async () => {
    await sendWithText(input);
    setInput("");
  }, [input, sendWithText]);

  const retryLast = useCallback(async () => {
    if (!retryUserText) return;
    await sendWithText(retryUserText);
  }, [retryUserText, sendWithText]);

  const newChat = useCallback(() => {
    setMessages([]);
    setBusyId(null);
    setRetryUserText(null);
    lastSentRef.current = null;
    setHighlightNodeIds([]);
    setPreview(null);
  }, [setHighlightNodeIds]);

  const lastAssistantError = messages.filter((m) => m.role === "assistant" && m.error).pop();

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-line px-3 py-2">
        <p className="font-medium text-text">Chat</p>
        <Button type="button" variant="quiet" onClick={newChat}>
          New chat
        </Button>
      </div>
      {health?.warmUpError ? (
        <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
          <p className="text-danger">{health.warmUpError}</p>
          <Button type="button" variant="secondary" onClick={() => void retryWarmUp()}>
            Retry
          </Button>
        </div>
      ) : null}
      <div className="flex min-h-0 flex-1 flex-col gap-2 p-3">
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto">
          {messages.map((m) => {
            const used =
              m.role === "assistant" && m.citations?.length
                ? citationsUsedInOrder(m.text, m.citations)
                : [];
            const showAllRetrieved =
              m.role === "assistant" &&
              !!m.citations?.length &&
              used.length === 0 &&
              !busyId;
            const chipCitations = showAllRetrieved ? m.citations! : used;
            return (
              <div
                key={m.id}
                className={
                  m.role === "user"
                    ? "ml-6 rounded-[6px] bg-pane p-2"
                    : "mr-4 rounded-[6px] border border-line p-2"
                }
              >
                {m.role === "assistant" ? (
                  <AssistantMarkdown
                    text={m.text || (busyId === m.id ? "…" : "")}
                    citations={m.citations ?? []}
                    onCite={(c) => void openCitation(c)}
                  />
                ) : (
                  <p className="whitespace-pre-wrap">{m.text}</p>
                )}
                {m.role === "assistant" && m.retrievalMode ? (
                  <p className="mt-1 text-muted">
                    {m.retrievalMode === "hybrid"
                      ? "Hybrid retrieval"
                      : m.retrievalMode === "vector_only"
                        ? "Vector-only (graph evidence not used)"
                        : m.retrievalMode}
                  </p>
                ) : null}
                {m.error ? (
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <p className="text-danger">{m.error}</p>
                    {retryUserText && m.id === lastAssistantError?.id ? (
                      <Button type="button" variant="secondary" onClick={() => void retryLast()}>
                        Retry
                      </Button>
                    ) : null}
                  </div>
                ) : null}
                {chipCitations.length > 0 ? (
                  <div className="mt-2 space-y-1">
                    {showAllRetrieved ? <p className="text-muted">Retrieved</p> : null}
                    <div className="flex flex-wrap gap-1">
                      {chipCitations.map((c, i) => (
                        <button
                          key={`${c.chunkId}-${i}`}
                          type="button"
                          className="rounded-[6px] border border-line bg-bg px-2 py-0.5 text-[12px] hover:bg-hover"
                          onClick={() => void openCitation(c)}
                        >
                          [{c.index}] {c.title.slice(0, 24)}
                          {c.pageStart != null ? ` · p.${c.pageStart}` : ""}
                        </button>
                      ))}
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })}
          <div ref={bottomRef} />
        </div>
        <div className="flex shrink-0 gap-2">
          <textarea
            className={cn(
              "min-h-[72px] flex-1 resize-none rounded-[6px] border border-line bg-bg p-2 text-[13px]",
              "placeholder:text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent",
            )}
            placeholder={
              ready
                ? "Ask about your library…"
                : health?.warmUpPending
                  ? "Loading model…"
                  : "Chat unavailable until the model is ready."
            }
            value={input}
            disabled={!ready || !!busyId}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
            aria-label="Chat message"
          />
          <Button type="button" variant="primary" disabled={!ready || !!busyId} onClick={() => void send()}>
            Send
          </Button>
        </div>
      </div>
      {preview ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4"
          role="dialog"
          aria-modal
        >
          <div className="max-h-[80vh] w-full max-w-lg overflow-auto rounded-[10px] border border-line bg-bg p-4 shadow-[0_8px_30px_rgba(0,0,0,0.12)]">
            <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
              <h3 className="font-medium text-text">{preview.title}</h3>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() => void openDocument(preview.documentId)}
                >
                  Open file
                </Button>
                <Button type="button" variant="quiet" onClick={() => setPreview(null)}>
                  Close
                </Button>
              </div>
            </div>
            <PreviewBody text={preview.text} highlight={preview.highlight} />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function AssistantMarkdown({
  text,
  citations,
  onCite,
}: {
  text: string;
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
}) {
  const indexes = new Set(citations.map((c) => c.index));
  const byIndex = new Map(citations.map((c) => [c.index, c]));
  const md = linkifyCitationMarkers(text, indexes);
  const components: Components = {
    a: ({ href, children }) => {
      if (href?.startsWith("#cite-")) {
        const n = Number(href.slice(6));
        const c = byIndex.get(n);
        return (
          <button
            type="button"
            className="mx-0.5 inline-flex h-5 min-w-5 items-center justify-center rounded-[6px] border border-line bg-pane px-1 align-super text-[10px] font-medium hover:bg-hover"
            onClick={() => {
              if (c) onCite(c);
            }}
            aria-label={`Open citation ${n}`}
          >
            {children}
          </button>
        );
      }
      return <span>{children}</span>;
    },
  };
  return (
    <div className="prose prose-sm max-w-none font-read text-[13px] text-text [&_p]:my-1 [&_ul]:my-1 [&_ol]:my-1">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {md}
      </ReactMarkdown>
    </div>
  );
}

export function PreviewBody({ text, highlight }: { text: string | null; highlight: string | null }) {
  if (!text) {
    return <p className="text-muted">No preview text.</p>;
  }
  if (!highlight) {
    return <p className="whitespace-pre-wrap font-read">{text}</p>;
  }
  let i = text.indexOf(highlight);
  if (i < 0) {
    const prefixLen = Math.min(40, highlight.length);
    const prefix = highlight.slice(0, prefixLen);
    i = text.indexOf(prefix);
    if (i < 0) {
      return <p className="whitespace-pre-wrap font-read">{text}</p>;
    }
  }
  const before = text.slice(0, i);
  const mid = text.slice(i, i + highlight.length);
  const after = text.slice(i + highlight.length);
  return (
    <p className="whitespace-pre-wrap font-read">
      {before}
      <mark className="bg-mark text-text">{mid}</mark>
      {after}
    </p>
  );
}
