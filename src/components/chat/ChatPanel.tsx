import { Button } from "@/components/ui/button";
import { SourceQuoteOrThumb } from "@/components/common/SourceRegionThumb";
import {
  getDocumentPreview,
  getSuggestedQuestions,
  listDocuments,
  prepareModels,
  sendChatQuery,
  sendRelatedQuery,
  warmChatModel,
  type AnswerSentence,
  type AppHealth,
  type ChatTurn,
  type CitationItem,
  type DocumentRow,
  type PageRegion,
  type SuggestedQuestion,
} from "@/lib/tauri";
import { useTauriEvent } from "@/lib/tauri-events";
import { cn } from "@/lib/utils";
import { useChatIntentStore, type ScopeDoc } from "@/stores/chatIntentStore";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";

type ScopeSearchSuggestion = {
  documentId: string;
  title: string;
  label?: string | null;
};

type ChatMessage = {
  id: string;
  role: "user" | "assistant";
  text: string;
  citations?: CitationItem[];
  sentences?: AnswerSentence[];
  notFound?: boolean;
  scopeTitle?: string | null;
  suggestions?: ScopeSearchSuggestion[];
  error?: string;
  stage?: "passages" | "ranking" | "writing" | "checking" | "done";
  stageDetail?: string;
};

type Props = {
  health: AppHealth | undefined;
};

export function citationSourceLabel(c: CitationItem): string {
  const name = (c.title || "").trim() || fileName(c.path);
  if (c.pageStart != null && c.pageEnd != null && c.pageEnd !== c.pageStart) {
    return `${name}, pp. ${c.pageStart}-${c.pageEnd}`;
  }
  if (c.pageStart != null) {
    return `${name}, p. ${c.pageStart}`;
  }
  return name;
}

/** Turn \\( \\) / \\[ \\] into $ / $$ outside code spans and blocks. */
export function normalizeAnswerMath(text: string): string {
  const parts = text.split(/(```[\s\S]*?```|`[^`\n]+`)/g);
  return parts
    .map((part) => {
      if (part.startsWith("`")) return part;
      return part
        .replace(/\\\(([\s\S]+?)\\\)/g, (_m, body: string) => `$${body}$`)
        .replace(/\\\[([\s\S]+?)\\\]/g, (_m, body: string) => `$$${body}$$`);
    })
    .join("");
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

/** Clamp snippet to at most three lines, starting at the best-matching sentence. */
export function clampSnippetToMatch(text: string, query: string, maxLines = 3): string {
  const raw = (text || "").replace(/\s+/g, " ").trim();
  if (!raw) return "";
  const qTerms = new Set(
    (query || "")
      .toLowerCase()
      .match(/[0-9a-z]+(?:'[0-9a-z]+)?/g)
      ?.filter((t) => t.length >= 2) ?? [],
  );
  const sentences = raw.split(/(?<=[.!?])\s+/).filter(Boolean);
  let bestIdx = 0;
  let bestScore = -1;
  for (let i = 0; i < sentences.length; i++) {
    const terms = sentences[i].toLowerCase().match(/[0-9a-z]+(?:'[0-9a-z]+)?/g) ?? [];
    let score = 0;
    for (const t of terms) {
      if (qTerms.has(t)) score += 1;
    }
    if (score > bestScore) {
      bestScore = score;
      bestIdx = i;
    }
  }
  const fromBest = sentences.slice(bestIdx).join(" ");
  const words = fromBest.split(/\s+/);
  const approxLine = 14;
  const maxWords = approxLine * maxLines;
  if (words.length <= maxWords) return fromBest;
  return `${words.slice(0, maxWords).join(" ")}…`;
}

function assistantHistoryText(m: ChatMessage): string {
  if (m.sentences && m.sentences.length > 0) {
    return m.sentences.map((s) => s.text).join(" ");
  }
  return m.text;
}

function atToken(value: string, cursor: number): { start: number; query: string } | null {
  const before = value.slice(0, cursor);
  const m = before.match(/@([^\s@]*)$/);
  if (!m) return null;
  return { start: before.length - m[0].length, query: m[1] ?? "" };
}

export function ChatPanel({ health }: Props) {
  const queryClient = useQueryClient();
  const openPassage = useSidePaneStore((s) => s.openPassage);
  const addCitedRegions = useCitedRegionsStore((s) => s.addRegions);
  const consumeIntent = useChatIntentStore((s) => s.consumeIntent);
  const pendingRelated = useChatIntentStore((s) => s.pendingRelated);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [retryUserText, setRetryUserText] = useState<string | null>(null);
  const [scopeDocs, setScopeDocs] = useState<ScopeDoc[]>([]);
  const [libraryDocs, setLibraryDocs] = useState<DocumentRow[]>([]);
  const [suggestions, setSuggestions] = useState<SuggestedQuestion[]>([]);
  const [scopePickerOpen, setScopePickerOpen] = useState(false);
  const [atFilter, setAtFilter] = useState("");
  const [preparingModels, setPreparingModels] = useState(false);
  const [modelsProgress, setModelsProgress] = useState<string | null>(null);
  const lastSentRef = useRef<string | null>(null);
  const lastSentScopeRef = useRef<ScopeDoc[]>([]);
  const warmRetryErrorRef = useRef<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const composerRef = useRef<HTMLDivElement>(null);

  const needsModels =
    health !== undefined && health.checkingModelsReady === false;

  const ready =
    !!health?.sidecarReachable &&
    !!health?.chatModelWarmed &&
    !health?.warmUpPending &&
    !!health?.vaultConfigured &&
    !!health?.checkingModelsReady;

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

  useEffect(() => {
    if (!scopePickerOpen) return;
    void listDocuments().then((docs) => {
      setLibraryDocs(docs.filter((d) => d.status === "indexed"));
    });
  }, [scopePickerOpen]);

  useEffect(() => {
    const intent = consumeIntent();
    if (intent.scope.length > 0) {
      setScopeDocs(intent.scope);
    }
    if (intent.question) {
      setInput(intent.question);
    }
    if (intent.scope.length > 0 || intent.question) {
      requestAnimationFrame(() => textareaRef.current?.focus());
    }
  }, [consumeIntent]);

  useEffect(() => {
    if (!pendingRelated || busyId) return;
    const related = useChatIntentStore.getState().consumeRelated();
    if (!related) return;
    const query = `How are ${related.nameA} and ${related.nameB} related?`;
    const userId = crypto.randomUUID();
    const assistantId = crypto.randomUUID();
    lastSentRef.current = query;
    lastSentScopeRef.current = [];
    setMessages((prev) => [
      ...prev,
      { id: userId, role: "user", text: query },
      { id: assistantId, role: "assistant", text: "" },
    ]);
    setBusyId(assistantId);
    setInput("");
    void sendRelatedQuery(related.entityIdA, related.entityIdB, assistantId).catch((e) => {
      const msg = String(e);
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId && m.role === "assistant" ? { ...m, error: msg } : m,
        ),
      );
      setBusyId(null);
      setRetryUserText(query);
    });
  }, [pendingRelated, busyId]);

  useEffect(() => {
    if (!ready || messages.length > 0) {
      setSuggestions([]);
      return;
    }
    let cancelled = false;
    void getSuggestedQuestions()
      .then((qs) => {
        if (!cancelled) setSuggestions(qs.slice(0, 5));
      })
      .catch(() => {
        if (!cancelled) setSuggestions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [ready, messages.length]);

  const applySuggestion = useCallback(async (q: SuggestedQuestion) => {
    setInput(q.question);
    if (q.scopeIds.length > 0) {
      const docs = await listDocuments();
      const byId = new Map(docs.map((d) => [d.id, d]));
      const scopes: ScopeDoc[] = [];
      for (const id of q.scopeIds) {
        const d = byId.get(id);
        if (!d) continue;
        scopes.push({ id: d.id, title: d.title.trim() || fileName(d.path) });
      }
      setScopeDocs(scopes);
    } else {
      setScopeDocs([]);
    }
    requestAnimationFrame(() => textareaRef.current?.focus());
  }, []);

  useEffect(() => {
    if (!scopePickerOpen) return;
    const onPointer = (e: MouseEvent) => {
      const root = composerRef.current;
      if (root && !root.contains(e.target as Node)) {
        setScopePickerOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointer);
    return () => document.removeEventListener("mousedown", onPointer);
  }, [scopePickerOpen]);

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

  useTauriEvent<{
    messageId: string;
    stage: string;
    citations?: CitationItem[] | null;
    detail?: string | null;
  }>("chat-stage", (ev) => {
    const p = ev.payload;
    const stage = p.stage as ChatMessage["stage"];
    if (
      stage !== "passages" &&
      stage !== "ranking" &&
      stage !== "writing" &&
      stage !== "checking"
    ) {
      return;
    }
    setMessages((prev) =>
      prev.map((m) =>
        m.id === p.messageId && m.role === "assistant"
          ? {
              ...m,
              stage,
              stageDetail: p.detail ?? undefined,
              citations: p.citations ?? m.citations,
            }
          : m,
      ),
    );
  });

  useTauriEvent<{
    messageId: string;
    citations: CitationItem[];
    sentences?: AnswerSentence[];
    notFound?: boolean;
    scopeTitle?: string | null;
    suggestions?: ScopeSearchSuggestion[];
    error?: string | null;
  }>("chat-final", (ev) => {
    const p = ev.payload;
    setBusyId(null);
    if (p.error) {
      setRetryUserText(lastSentRef.current);
    } else {
      setRetryUserText(null);
    }
    const sentences = p.sentences ?? [];
    setMessages((prev) =>
      prev.map((m) =>
        m.id === p.messageId && m.role === "assistant"
          ? {
              ...m,
              citations: p.citations,
              sentences,
              notFound: p.notFound ?? false,
              scopeTitle: p.scopeTitle ?? null,
              suggestions: p.suggestions ?? [],
              error: p.error ?? undefined,
              stage: "done",
              stageDetail: undefined,
              text: sentences.map((s) => s.text).join(" "),
            }
          : m,
      ),
    );
  });

  useTauriEvent<{ line: string }>("models-progress", (ev) => {
    setModelsProgress(ev.payload.line);
  });

  const openCitation = useCallback(
    async (c: CitationItem) => {
      const prev = await getDocumentPreview(c.documentId, c.chunkId);
      const start = prev.pageStart ?? c.pageStart;
      const end = prev.pageEnd ?? c.pageEnd;
      const regions = (prev.regions?.length ? prev.regions : c.regions) ?? null;
      const path = prev.path || c.path;
      const page = start ?? regions?.[0]?.page;
      addCitedRegions(prev.documentId, regions);
      openPassage({
        documentId: prev.documentId,
        title: (c.title || "").trim() || fileName(c.path),
        pageLabel: pageLabel(start, end),
        text: prev.text,
        highlight: prev.highlightSnippet ?? c.snippet,
        path,
        page: page ?? undefined,
        pageCount: null,
        regions,
      });
    },
    [addCitedRegions, openPassage],
  );

  const downloadModels = useCallback(async () => {
    setPreparingModels(true);
    setModelsProgress(null);
    try {
      await prepareModels();
      await queryClient.invalidateQueries({ queryKey: ["health"] });
    } finally {
      setPreparingModels(false);
    }
  }, [queryClient]);

  const sendWithText = useCallback(
    async (q: string, scopeOverride?: ScopeDoc[]) => {
      const query = q.trim();
      if (!query || !ready || busyId) return;
      const scopes = scopeOverride ?? scopeDocs;
      setRetryUserText(null);
      lastSentRef.current = query;
      lastSentScopeRef.current = scopes;
      const prior = messages.filter((m) => !m.error);
      const historySource = prior.slice(-4);
      const history: ChatTurn[] = historySource
        .map((m) => ({
          role: m.role,
          content: m.role === "assistant" ? assistantHistoryText(m) : m.text,
        }))
        .filter((t) => t.content.trim().length > 0);
      const userId = crypto.randomUUID();
      const assistantId = crypto.randomUUID();
      setMessages((prev) => [
        ...prev,
        { id: userId, role: "user", text: query },
        { id: assistantId, role: "assistant", text: "" },
      ]);
      setBusyId(assistantId);
      try {
        await sendChatQuery(
          query,
          assistantId,
          history,
          scopes.map((d) => d.id),
        );
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
    [ready, busyId, messages, scopeDocs],
  );

  const send = useCallback(async () => {
    await sendWithText(input);
    setInput("");
    setScopePickerOpen(false);
  }, [input, sendWithText]);

  const retryLast = useCallback(async () => {
    if (!retryUserText) return;
    await sendWithText(retryUserText, lastSentScopeRef.current);
  }, [retryUserText, sendWithText]);

  const newChat = useCallback(() => {
    setMessages([]);
    setBusyId(null);
    setRetryUserText(null);
    lastSentRef.current = null;
    lastSentScopeRef.current = [];
  }, []);

  const pickScopeDoc = useCallback(
    (doc: DocumentRow) => {
      setScopeDocs((prev) =>
        prev.some((d) => d.id === doc.id)
          ? prev
          : [...prev, { id: doc.id, title: doc.title || fileName(doc.path) }],
      );
      const el = textareaRef.current;
      const cursor = el?.selectionStart ?? input.length;
      const token = atToken(input, cursor);
      if (token) {
        const next = input.slice(0, token.start) + input.slice(cursor);
        setInput(next);
      }
      setScopePickerOpen(false);
      setAtFilter("");
    },
    [input],
  );

  const onInputChange = useCallback((value: string) => {
    setInput(value);
    const el = textareaRef.current;
    const cursor = el?.selectionStart ?? value.length;
    const token = atToken(value, cursor);
    if (token) {
      setScopePickerOpen(true);
      setAtFilter(token.query);
    } else {
      setScopePickerOpen(false);
      setAtFilter("");
    }
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

  const scopeChoices = useMemo(() => {
    const q = atFilter.trim().toLowerCase();
    const chosen = new Set(scopeDocs.map((d) => d.id));
    return libraryDocs
      .filter((d) => !chosen.has(d.id))
      .filter((d) => {
        if (!q) return true;
        const title = (d.title || fileName(d.path)).toLowerCase();
        return title.includes(q);
      })
      .slice(0, 8);
  }, [libraryDocs, scopeDocs, atFilter]);

  const placeholder = !health?.vaultConfigured
    ? "Choose a folder first"
    : needsModels
      ? "Download checking models to ask questions"
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
      {needsModels ? (
        <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
          <p className="text-muted">
            Cognito needs two checking models, about 1 GB, downloaded once.
          </p>
          <Button
            type="button"
            variant="secondary"
            disabled={preparingModels}
            onClick={() => void downloadModels()}
          >
            {preparingModels ? "Downloading…" : "Download"}
          </Button>
          {modelsProgress ? <p className="text-muted">{modelsProgress}</p> : null}
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
            {messages.length === 0 && ready && suggestions.length > 0 ? (
              <div className="space-y-2">
                <p className="text-muted">Suggested questions</p>
                <ul className="space-y-1">
                  {suggestions.map((q) => (
                    <li key={q.question}>
                      <button
                        type="button"
                        className="text-left text-muted hover:text-text"
                        onClick={() => void applySuggestion(q)}
                      >
                        {q.question}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
            {turns.map(({ user, assistant }) => {
              const waiting =
                !!assistant &&
                busyId === assistant.id &&
                !assistant.stage &&
                !assistant.error &&
                !assistant.sentences?.length &&
                !assistant.notFound;

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
                  {assistant && !assistant.error ? (
                    <AssistantTurn
                      message={assistant}
                      query={user.text}
                      onCite={(c) => void openCitation(c)}
                      onAskEverywhere={() => void sendWithText(user.text, [])}
                      onSearchIn={(s) => {
                        const scope = [{ id: s.documentId, title: s.title }];
                        setScopeDocs(scope);
                        void sendWithText(user.text, scope);
                      }}
                    />
                  ) : null}
                </article>
              );
            })}
            <div ref={bottomRef} />
          </div>
        </div>
        <div ref={composerRef} className="relative shrink-0 pb-4 pt-2">
          <div className="mb-1.5 flex flex-wrap items-center gap-x-2 gap-y-1">
            {scopeDocs.length === 0 ? (
              <p className="text-muted">Searching all documents</p>
            ) : (
              scopeDocs.map((d) => (
                <span key={d.id} className="inline-flex items-center gap-1 text-muted">
                  In: {d.title}
                  <Button
                    type="button"
                    variant="quiet"
                    size="sm"
                    className="h-auto px-1 py-0 text-muted"
                    aria-label={`Remove ${d.title} from scope`}
                    onClick={() => setScopeDocs((prev) => prev.filter((x) => x.id !== d.id))}
                  >
                    ×
                  </Button>
                </span>
              ))
            )}
          </div>
          {scopePickerOpen && scopeChoices.length > 0 ? (
            <ul
              className="absolute bottom-full z-10 mb-1 max-h-48 w-full overflow-y-auto rounded-[8px] border border-line bg-bg py-1 shadow-sm"
              role="listbox"
              aria-label="Document scope"
            >
              {scopeChoices.map((d) => {
                const title = d.title || fileName(d.path);
                return (
                  <li key={d.id}>
                    <button
                      type="button"
                      role="option"
                      className="w-full px-3 py-1.5 text-left font-ui text-[14px] text-text hover:bg-hover"
                      onClick={() => pickScopeDoc(d)}
                    >
                      {title}
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : null}
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
            onChange={(e) => onInputChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && scopePickerOpen) {
                e.preventDefault();
                setScopePickerOpen(false);
                return;
              }
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

function SourcesList({
  citations,
  onCite,
}: {
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
}) {
  const [enriched, setEnriched] = useState<CitationItem[]>(citations);

  useEffect(() => {
    setEnriched(citations);
    let cancelled = false;
    void (async () => {
      const next = await Promise.all(
        citations.map(async (c) => {
          if (c.regions && c.regions.length > 0) return c;
          try {
            const prev = await getDocumentPreview(c.documentId, c.chunkId);
            const regions: PageRegion[] | null | undefined = prev.regions;
            if (!regions?.length && !prev.path) return c;
            return {
              ...c,
              regions: regions?.length ? regions : c.regions,
              path: prev.path || c.path,
              pageStart: prev.pageStart ?? c.pageStart,
              pageEnd: prev.pageEnd ?? c.pageEnd,
            };
          } catch {
            return c;
          }
        }),
      );
      if (!cancelled) setEnriched(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [citations]);

  if (citations.length === 0) return null;
  return (
    <div className="space-y-2">
      <p className="text-muted">Sources</p>
      <ol className="list-decimal space-y-2 pl-5 text-muted">
        {enriched.map((c) => (
          <li key={`${c.chunkId}-${c.index}`} className="space-y-1">
            {c.regions && c.regions.length > 0 && c.path ? (
              <SourceQuoteOrThumb
                path={c.path}
                regions={c.regions}
                quote={c.snippet}
                documentId={c.documentId}
                title={(c.title || "").trim() || fileName(c.path)}
              />
            ) : (
              <p className="text-muted">{c.snippet}</p>
            )}
            <button
              type="button"
              className="text-left text-muted hover:text-text"
              onClick={() => onCite(c)}
            >
              {citationSourceLabel(c)}
            </button>
          </li>
        ))}
      </ol>
    </div>
  );
}

function ClosestPassages({
  citations,
  query,
  onCite,
}: {
  citations: CitationItem[];
  query: string;
  onCite: (c: CitationItem) => void;
}) {
  const [enriched, setEnriched] = useState<CitationItem[]>(citations);

  useEffect(() => {
    setEnriched(citations);
    let cancelled = false;
    void (async () => {
      const next = await Promise.all(
        citations.map(async (c) => {
          if (c.regions && c.regions.length > 0) return c;
          try {
            const prev = await getDocumentPreview(c.documentId, c.chunkId);
            const regions: PageRegion[] | null | undefined = prev.regions;
            if (!regions?.length && !prev.path) return c;
            return {
              ...c,
              regions: regions?.length ? regions : c.regions,
              path: prev.path || c.path,
              pageStart: prev.pageStart ?? c.pageStart,
              pageEnd: prev.pageEnd ?? c.pageEnd,
            };
          } catch {
            return c;
          }
        }),
      );
      if (!cancelled) setEnriched(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [citations]);

  if (citations.length === 0) return null;
  return (
    <ul className="space-y-3">
      {enriched.map((c) => {
        const title = (c.title || "").trim() || fileName(c.path);
        const page = pageLabel(c.pageStart, c.pageEnd);
        const snippet = clampSnippetToMatch(c.snippet || "", query);
        return (
          <li key={`${c.chunkId}-${c.index}`}>
            <button
              type="button"
              className="w-full space-y-1 text-left text-muted hover:text-text"
              onClick={() => onCite(c)}
            >
              <p className="text-muted">
                <span>{title}</span>
                {page ? (
                  <>
                    <span aria-hidden="true"> · </span>
                    <span>{page}</span>
                  </>
                ) : null}
              </p>
              {snippet ? <p className="text-muted">{snippet}</p> : null}
              {c.regions && c.regions.length > 0 && c.path ? (
                <SourceQuoteOrThumb
                  path={c.path}
                  regions={c.regions}
                  quote={c.snippet}
                  documentId={c.documentId}
                  title={title}
                />
              ) : null}
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function AssistantTurn({
  message,
  query,
  onCite,
  onAskEverywhere,
  onSearchIn,
}: {
  message: ChatMessage;
  query: string;
  onCite: (c: CitationItem) => void;
  onAskEverywhere: () => void;
  onSearchIn: (s: ScopeSearchSuggestion) => void;
}) {
  const citations = message.citations ?? [];
  const stage = message.stage;
  const ranking =
    stage === "ranking" ? message.stageDetail || "Ranking passages" : null;
  const writing =
    stage === "passages" || stage === "writing"
      ? message.stageDetail || "Writing the answer"
      : null;
  const checking =
    stage === "checking" ? message.stageDetail || "Checking sentences" : null;

  if (message.notFound) {
    const searchSuggestions = message.suggestions ?? [];
    return (
      <div className="space-y-3">
        <p className="text-muted">Your documents don&apos;t seem to cover this.</p>
        <ClosestPassages citations={citations} query={query} onCite={onCite} />
        {searchSuggestions.length > 0 ? (
          <ul className="space-y-1">
            {searchSuggestions.map((s) => (
              <li key={s.documentId}>
                <button
                  type="button"
                  className="text-left text-muted hover:text-text"
                  onClick={() => onSearchIn(s)}
                >
                  {s.label || `Search in ${s.title}`}
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    );
  }

  if (stage === "done" && message.sentences && message.sentences.length > 0) {
    return (
      <div className="space-y-3">
        {message.scopeTitle ? (
          <p className="text-muted">
            Answered from {message.scopeTitle}{" "}
            <Button
              type="button"
              variant="quiet"
              className="h-auto px-1 py-0 text-muted underline-offset-2 hover:underline"
              onClick={onAskEverywhere}
            >
              Ask across everything
            </Button>
          </p>
        ) : null}
        <div className="space-y-2">
          {message.sentences.map((s, i) => (
            <SentenceBlock key={i} sentence={s} citations={citations} onCite={onCite} />
          ))}
        </div>
        <SourcesList citations={citations} onCite={onCite} />
      </div>
    );
  }

  if (writing || checking || ranking || citations.length > 0) {
    return (
      <div className="space-y-3">
        {citations.length > 0 &&
        (stage === "passages" ||
          stage === "ranking" ||
          stage === "writing" ||
          stage === "checking") ? (
          <SourcesList citations={citations} onCite={onCite} />
        ) : null}
        {ranking ? <p className="text-muted">{ranking}</p> : null}
        {writing ? <p className="text-muted">{writing}</p> : null}
        {checking ? <p className="text-muted">{checking}</p> : null}
      </div>
    );
  }

  return null;
}

function SentenceBlock({
  sentence,
  citations,
  onCite,
}: {
  sentence: AnswerSentence;
  citations: CitationItem[];
  onCite: (c: CitationItem) => void;
}) {
  const byIndex = new Map(citations.map((c) => [c.index, c]));
  const unsupported = sentence.status === "unsupported";
  const unchecked = sentence.status === "unchecked";
  const muted = unsupported || unchecked;
  return (
    <div className={cn("space-y-0.5", muted && "text-muted")}>
      <div
        className={cn(
          "answer font-read text-[15px] leading-[1.6]",
          muted ? "text-muted" : "text-text",
        )}
      >
        <ReactMarkdown
          remarkPlugins={[remarkGfm, remarkMath]}
          rehypePlugins={[[rehypeKatex, { throwOnError: false }]]}
        >
          {normalizeAnswerMath(sentence.text)}
        </ReactMarkdown>
        {sentence.sources.map((n) => {
          const c = byIndex.get(n);
          return (
            <sup key={n}>
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
        })}
      </div>
      {unsupported ? (
        <p className="text-[12px] text-muted">Not found in the sources</p>
      ) : null}
      {unchecked ? <p className="text-[12px] text-muted">Not checked</p> : null}
    </div>
  );
}
