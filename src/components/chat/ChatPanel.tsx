import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ChatWelcome } from "@/components/chat/ChatWelcome";
import { useRetrievalMode } from "@/context/RetrievalModeContext";
import {
  getDocumentPreview,
  sendChatQuery,
  type AppHealth,
  type CitationItem,
} from "@/lib/tauri";
import { listen } from "@/lib/tauri-events";
import { useGraphSessionStore } from "@/stores/graphSessionStore";
import { cn } from "@/lib/utils";
import { useCallback, useEffect, useRef, useState } from "react";

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

export function ChatPanel({ health }: Props) {
  const { setLastRetrievalMode } = useRetrievalMode();
  const setHighlightNodeIds = useGraphSessionStore((s) => s.setHighlightNodeIds);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [retryUserText, setRetryUserText] = useState<string | null>(null);
  const lastSentRef = useRef<string | null>(null);
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

  const sendWithText = useCallback(
    async (q: string) => {
      const query = q.trim();
      if (!query || !ready || busyId) return;
      setRetryUserText(null);
      lastSentRef.current = query;
      const userId = crypto.randomUUID();
      const assistantId = await sendChatQuery(query);
      setMessages((prev) => [
        ...prev,
        { id: userId, role: "user", text: query },
        { id: assistantId, role: "assistant", text: "" },
      ]);
      setBusyId(assistantId);
    },
    [ready, busyId],
  );

  const send = useCallback(async () => {
    await sendWithText(input);
    setInput("");
  }, [input, sendWithText]);

  const retryLast = useCallback(async () => {
    if (!retryUserText) return;
    await sendWithText(retryUserText);
  }, [retryUserText, sendWithText]);

  const openCitation = useCallback(
    async (c: CitationItem) => {
      setHighlightNodeIds(c.nodeIds ?? []);
      const prev = await getDocumentPreview(c.documentId, c.snippet.slice(0, 400));
      setPreview({
        documentId: prev.documentId,
        text: prev.text,
        highlight: prev.highlightSnippet ?? c.snippet,
        title: c.title,
      });
    },
    [setHighlightNodeIds],
  );

  const lastAssistantError = messages.filter((m) => m.role === "assistant" && m.error).pop();

  return (
    <Card className="flex h-full min-h-0 flex-col">
      <CardHeader className="shrink-0 pb-2">
        <div className="flex items-start justify-between gap-2">
          <CardTitle>Chat</CardTitle>
        </div>
        {health?.warmUpError ? (
          <p className="text-xs text-amber-600 dark:text-amber-400">{health.warmUpError}</p>
        ) : null}
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-2">
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto text-sm">
          {messages.length === 0 ? <ChatWelcome modelReady={ready} /> : null}
          {messages.map((m) => (
            <div
              key={m.id}
              className={
                m.role === "user"
                  ? "ml-6 rounded-md bg-[var(--surface)] p-2"
                  : "mr-4 rounded-md border border-[var(--border)] p-2"
              }
            >
              <p className="whitespace-pre-wrap">{m.text || (busyId === m.id ? "…" : "")}</p>
              {m.role === "assistant" && m.retrievalMode ? (
                <p
                  className={cn(
                    "mt-1 text-[10px] uppercase",
                    m.retrievalMode === "hybrid" ? "text-sky-300/90" : "text-[var(--muted)]",
                  )}
                >
                  {m.retrievalMode === "hybrid"
                    ? "Hybrid retrieval"
                    : m.retrievalMode === "vector_only"
                      ? "Vector-only (graph evidence not used)"
                      : m.retrievalMode}
                </p>
              ) : null}
              {m.error ? (
                <div className="mt-2 flex flex-wrap items-center gap-2">
                  <p className="text-xs text-red-500">{m.error}</p>
                  {retryUserText && m.id === lastAssistantError?.id ? (
                    <Button type="button" variant="outline" size="sm" onClick={() => void retryLast()}>
                      Retry
                    </Button>
                  ) : null}
                </div>
              ) : null}
              {m.citations && m.citations.length > 0 ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {m.citations.map((c, i) => (
                    <button
                      key={`${c.chunkId}-${i}`}
                      type="button"
                      className="rounded-full border border-[var(--border)] bg-[var(--background)] px-2 py-0.5 text-[10px] hover:bg-[var(--surface)]"
                      onClick={() => void openCitation(c)}
                    >
                      {c.title.slice(0, 24)}
                      {c.pageStart != null ? ` · p.${c.pageStart}` : ""}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          ))}
          <div ref={bottomRef} />
        </div>
        <div className="flex shrink-0 gap-2">
          <textarea
            className="min-h-[72px] flex-1 resize-none rounded-md border border-[var(--border)] bg-[var(--background)] p-2 text-sm"
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
          <Button type="button" disabled={!ready || !!busyId} onClick={() => void send()}>
            Send
          </Button>
        </div>
      </CardContent>
      {preview ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
          role="dialog"
          aria-modal
        >
          <div className="max-h-[80vh] w-full max-w-lg overflow-auto rounded-lg border border-[var(--border)] bg-[var(--background)] p-4 shadow-lg">
            <div className="mb-2 flex justify-between gap-2">
              <h3 className="text-sm font-medium">{preview.title}</h3>
              <button
                type="button"
                className="text-xs text-[var(--muted)]"
                onClick={() => setPreview(null)}
              >
                Close
              </button>
            </div>
            <PreviewBody text={preview.text} highlight={preview.highlight} />
          </div>
        </div>
      ) : null}
    </Card>
  );
}

function PreviewBody({ text, highlight }: { text: string | null; highlight: string | null }) {
  if (!text) {
    return <p className="text-xs text-[var(--muted)]">No preview text.</p>;
  }
  if (!highlight || !text.includes(highlight.slice(0, 20))) {
    return <p className="whitespace-pre-wrap text-xs">{text}</p>;
  }
  const i = text.indexOf(highlight.slice(0, Math.min(40, highlight.length)));
  if (i < 0) {
    return <p className="whitespace-pre-wrap text-xs">{text}</p>;
  }
  const before = text.slice(0, i);
  const mid = text.slice(i, i + highlight.length);
  const after = text.slice(i + highlight.length);
  return (
    <p className="whitespace-pre-wrap text-xs">
      {before}
      <mark className="bg-yellow-200/40 dark:bg-yellow-500/30">{mid}</mark>
      {after}
    </p>
  );
}
