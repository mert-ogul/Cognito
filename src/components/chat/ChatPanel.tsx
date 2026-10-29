import { Button } from "@/components/ui/button";
import {
  getDocumentPreview,
  sendChatQuery,
  warmChatModel,
  type AppHealth,
  type ChatTurn,
  type CitationItem,
} from "@/lib/tauri";
import { listen } from "@/lib/tauri-events";
import { cn } from "@/lib/utils";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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

export function citationSourceLabel(c: CitationItem): string {
  const name = fileName(c.path) || c.title;
  if (c.pageStart != null && c.pageEnd != null && c.pageEnd !== c.pageStart) {
    return `${name}, pp. ${c.pageStart}-${c.pageEnd}`;
  }
  if (c.pageStart != null) {
    return `${name}, p. ${c.pageStart}`;
  }
  return name;
}

function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

function pageLabel(pageStart: number | null | undefined, pageEnd: number | null | undefined): string | null {
  if (pageStart != null && pageEnd != null && pageEnd !== pageStart) {
    return `pp. ${pageStart}-${pageEnd}`;
  }
  if (pageStart != null) {
    return `p. ${pageStart}`;
  }
  return null;
}

export function ChatPanel({ health }: Props) {
  const queryClient = useQueryClient();
  const openPassage = useSidePaneStore((s) => s.openPassage);
  const openGraphWithHighlights = useSidePaneStore((s) => s.openGraphWithHighlights);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [retryUserText, setRetryUserText] = useState<string | null>(null);
  const lastSentRef = useRef<string | null>(null);
  const warmRetryErrorRef = useRef<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const ready =
    !!health?.sidecarReachable &&
    !!health?.chatModelWarmed &&
    !health?.warmUpPending &&
    !!health?.vaultConfigured;

  useEffect(() => {
    const el = bottomRef.current;
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ block: "end" });
    }
  }, [messages, busyId]);

  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const line = 20;
    const max = line * 8 + 16;
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [input]);

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
  }, []);

  const openCitation = useCallback(
    async (c: CitationItem) => {
      const prev = await getDocumentPreview(c.documentId, c.chunkId);
      const start = prev.pageStart ?? c.pageStart;
      const end = prev.pageEnd ?? c.pageEnd;
      openPassage({
        documentId: prev.documentId,
        title: fileName(c.path) || c.title,
        pageLabel: pageLabel(start, end),
        text: prev.text,
        highlight: prev.highlightSnippet ?? c.snippet,
      });
    },
    [openPassage],
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
  }, []);

  const turns = useMemo(() => {
    const out: { user: ChatMessage; assistant: ChatMessage | null }[] = [];
    for (let i = 0; i < messages.length; i++) {
      const m = messages[i];
      if (m.role !== "user") continue;
      const next = messages[i + 1];
      out.push({
        user: m,
        assistant: next?.role === "assistant" ? next : null,
      });
    }
    return out;
  }, [messages]);

  const lastAssistantError = messages.filter((m) => m.role === "assistant" && m.error).pop();

  const placeholder = !health?.vaultConfigured
    ? "Choose a folder first"
    : health?.warmUpPending
      ? `Loading ${health.chatModel}`
      : ready
        ? "Ask about your library"
        : "Chat unavailable until the model is ready.";

  return (
    <div className="flex h-full min-h-0 flex-col">
      {health?.warmUpError ? (
        <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
          <p className="text-danger">{health.warmUpError}</p>
          <Button type="button" variant="quiet" onClick={() => void retryWarmUp()}>
            Try again
          </Button>
        </div>
      ) : null}
      <div className="mx-auto flex min-h-0 w-full max-w-[68ch] flex-1 flex-col px-4 pt-8">
        <div className="mb-2 flex min-h-7 justify-end">
          {messages.length > 0 ? (
            <Button type="button" variant="quiet" onClick={newChat}>
              New chat
            </Button>
          ) : null}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="space-y-8 pb-8">
            {turns.map(({ user, assistant }) => {
              const waiting =
                !!assistant &&
                busyId === assistant.id &&
                !assistant.text &&
                !assistant.error;
              const used =
                assistant && assistant.citations?.length && !waiting
                  ? citationsUsedInOrder(assistant.text, assistant.citations)
                  : [];
              const showRetrieved =
                !!assistant &&
                !!assistant.citations?.length &&
                used.length === 0 &&
                !busyId &&
                !assistant.error;
              const listCitations = showRetrieved ? assistant!.citations! : used;
              const listHeading = showRetrieved ? "Retrieved passages" : "Sources";
              const relatedIds = Array.from(
                new Set(
                  (used.length > 0 ? used : assistant?.citations ?? []).flatMap(
                    (c) => c.nodeIds ?? [],
                  ),
                ),
              );
              const showRelated =
                assistant?.retrievalMode === "hybrid" &&
                relatedIds.length > 0 &&
                !assistant.error &&
                !waiting;

              return (
                <article key={user.id} className="space-y-2">
                  <p className="font-ui text-[15px] font-semibold text-text">{user.text}</p>
                  {assistant?.error ? (
                    <div className="space-y-1">
                      <p className="text-danger">{assistant.error}</p>
                      {retryUserText && assistant.id === lastAssistantError?.id ? (
                        <Button type="button" variant="quiet" onClick={() => void retryLast()}>
                          Try again
                        </Button>
                      ) : null}
                    </div>
                  ) : null}
                  {waiting ? <p className="text-muted">Searching your library</p> : null}
                  {assistant && !assistant.error && assistant.text ? (
                    <div className="space-y-3">
                      <AssistantMarkdown
                        text={assistant.text}
                        citations={assistant.citations ?? []}
                        onCite={(c) => void openCitation(c)}
                      />
                      {listCitations.length > 0 ? (
                        <div className="space-y-1">
                          <p className="text-muted">{listHeading}</p>
                          <ol className="list-decimal space-y-0.5 pl-5 text-muted">
                            {listCitations.map((c) => (
                              <li key={`${c.chunkId}-${c.index}`}>
                                <button
                                  type="button"
                                  className="text-left text-muted hover:text-text"
                                  onClick={() => void openCitation(c)}
                                >
                                  {citationSourceLabel(c)}
                                </button>
                              </li>
                            ))}
                          </ol>
                        </div>
                      ) : null}
                      {showRelated ? (
                        <Button
                          type="button"
                          variant="quiet"
                          className="px-0"
                          onClick={() => openGraphWithHighlights(relatedIds)}
                        >
                          Show related entities in the graph
                        </Button>
                      ) : null}
                    </div>
                  ) : null}
                </article>
              );
            })}
            <div ref={bottomRef} />
          </div>
        </div>
        <div className="shrink-0 pb-4 pt-2">
          <textarea
            ref={textareaRef}
            rows={1}
            className={cn(
              "w-full resize-none overflow-y-auto rounded-[8px] border border-line bg-bg px-3 py-2 font-ui text-[15px] leading-[20px]",
              "placeholder:text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent",
            )}
            placeholder={placeholder}
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
        </div>
      </div>
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
          <sup>
            <button
              type="button"
              className="font-ui text-accent"
              onClick={() => {
                if (c) onCite(c);
              }}
              aria-label={`Source ${n}`}
            >
              {n}
            </button>
          </sup>
        );
      }
      return <span>{children}</span>;
    },
  };
  return (
    <div className="answer font-read text-[15px] leading-[1.6] text-text">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {md}
      </ReactMarkdown>
    </div>
  );
}
