import {
  AssistantMessage,
  citationSourceLabel,
  normalizeAnswerMath,
  type ScopeSearchSuggestion,
} from "@/components/chat/AssistantMessage";
import { Button } from "@/components/ui/button";
import {
  conversationTitle,
  storedMessageToChatMessage,
  type ChatMessage,
} from "@/lib/chatMessage";
import {
  createConversation,
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
  type SuggestedQuestion,
  getConversation,
} from "@/lib/tauri";
import { useTauriEvent } from "@/lib/tauri-events";
import { cn } from "@/lib/utils";
import { useChatIntentStore, type ScopeDoc } from "@/stores/chatIntentStore";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";
import { useConversationStore } from "@/stores/conversationStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

export { citationSourceLabel, normalizeAnswerMath };

type Props = {
  health: AppHealth | undefined;
  loadConversationId?: string | null;
  newChatEpoch?: number;
  onStartedNewChat?: () => void;
};

function formatModelBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 MB";
  const gb = n / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = n / (1024 * 1024);
  return `${Math.max(1, Math.round(mb))} MB`;
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

export function ChatPanel({
  health,
  loadConversationId = null,
  newChatEpoch = 0,
  onStartedNewChat,
}: Props) {
  const queryClient = useQueryClient();
  const openPassage = useSidePaneStore((s) => s.openPassage);
  const addCitedRegions = useCitedRegionsStore((s) => s.addRegions);
  const consumeIntent = useChatIntentStore((s) => s.consumeIntent);
  const pendingRelated = useChatIntentStore((s) => s.pendingRelated);
  const libraryResetEpoch = useChatIntentStore((s) => s.libraryResetEpoch);
  const activeId = useConversationStore((s) => s.activeId);
  const setActiveId = useConversationStore((s) => s.setActiveId);
  const busyMessageId = useConversationStore((s) => s.busyMessageId);
  const busyConversationId = useConversationStore((s) => s.busyConversationId);
  const setBusy = useConversationStore((s) => s.setBusy);
  const clearBusyIf = useConversationStore((s) => s.clearBusyIf);
  const resetActive = useConversationStore((s) => s.resetActive);

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
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
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
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

  const anyBusy = !!busyMessageId;
  const otherChatBusy =
    !!busyConversationId && !!activeId && busyConversationId !== activeId;

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      stickToBottomRef.current =
        el.scrollHeight - el.scrollTop - el.clientHeight <= 80;
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !stickToBottomRef.current) return;
    el.scrollTop = el.scrollHeight;
  }, [messages, busyMessageId]);

  const clearLocalChat = useCallback(() => {
    setMessages([]);
    setRetryUserText(null);
    setScopeDocs([]);
    lastSentRef.current = null;
    lastSentScopeRef.current = [];
  }, []);

  const startNewChat = useCallback(() => {
    setActiveId(null);
    clearLocalChat();
    onStartedNewChat?.();
  }, [clearLocalChat, onStartedNewChat, setActiveId]);

  useEffect(() => {
    if (libraryResetEpoch === 0) return;
    resetActive();
    clearLocalChat();
    void queryClient.invalidateQueries({ queryKey: ["conversations"] });
  }, [libraryResetEpoch, resetActive, clearLocalChat, queryClient]);

  const lastNewEpochRef = useRef(0);
  useEffect(() => {
    if (newChatEpoch === 0 || newChatEpoch === lastNewEpochRef.current) return;
    lastNewEpochRef.current = newChatEpoch;
    startNewChat();
  }, [newChatEpoch, startNewChat]);

  useEffect(() => {
    if (!loadConversationId) return;
    let cancelled = false;
    void getConversation(loadConversationId).then((detail) => {
      if (cancelled || !detail) return;
      setActiveId(detail.id);
      setMessages(detail.messages.map(storedMessageToChatMessage));
      setRetryUserText(null);
      setScopeDocs([]);
      lastSentRef.current = null;
      lastSentScopeRef.current = [];
    });
    return () => {
      cancelled = true;
    };
  }, [loadConversationId, setActiveId]);

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

  const ensureConversation = useCallback(
    async (firstText: string): Promise<string> => {
      const existing = useConversationStore.getState().activeId;
      if (existing) return existing;
      const created = await createConversation(conversationTitle(firstText));
      setActiveId(created.id);
      void queryClient.invalidateQueries({ queryKey: ["conversations"] });
      return created.id;
    },
    [queryClient, setActiveId],
  );

  useEffect(() => {
    if (!pendingRelated || anyBusy) return;
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
    void (async () => {
      try {
        const conversationId = await ensureConversation(query);
        setBusy(assistantId, conversationId);
        setInput("");
        await sendRelatedQuery(
          related.entityIdA,
          related.entityIdB,
          assistantId,
          conversationId,
        );
        void queryClient.invalidateQueries({ queryKey: ["conversations"] });
      } catch (e) {
        const msg = String(e);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant" ? { ...m, error: msg } : m,
          ),
        );
        clearBusyIf(assistantId);
        setRetryUserText(query);
      }
    })();
  }, [pendingRelated, anyBusy, ensureConversation, setBusy, clearBusyIf, queryClient]);

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
    conversationId?: string;
    stage: string;
    citations?: CitationItem[] | null;
    detail?: string | null;
    draft?: string[];
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
    const openId = useConversationStore.getState().activeId;
    if (p.conversationId && openId && p.conversationId !== openId) {
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
              draft: p.draft && p.draft.length > 0 ? p.draft : m.draft,
            }
          : m,
      ),
    );
  });

  useTauriEvent<{
    messageId: string;
    conversationId?: string;
    citations: CitationItem[];
    sentences?: AnswerSentence[];
    notFound?: boolean;
    scopeTitle?: string | null;
    suggestions?: ScopeSearchSuggestion[];
    error?: string | null;
  }>("chat-final", (ev) => {
    const p = ev.payload;
    clearBusyIf(p.messageId);
    void queryClient.invalidateQueries({ queryKey: ["conversations"] });
    const openId = useConversationStore.getState().activeId;
    if (p.conversationId && openId && p.conversationId !== openId) {
      return;
    }
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
              draft: undefined,
              text: sentences.map((s) => s.text).join(" "),
            }
          : m,
      ),
    );
  });

  useTauriEvent<{ downloadedBytes: number; totalBytes: number }>("models-progress", (ev) => {
    const { downloadedBytes, totalBytes } = ev.payload;
    setModelsProgress(
      `Downloading models, ${formatModelBytes(downloadedBytes)} of ${formatModelBytes(totalBytes)}`,
    );
  });

  const openCitation = useCallback(
    async (c: CitationItem, assistantId: string) => {
      try {
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
      } catch (e) {
        const msg = String(e).toLowerCase();
        if (msg.includes("not found")) {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId ? { ...m, sourceChanged: true } : m,
            ),
          );
          return;
        }
        throw e;
      }
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
      if (!query || !ready || anyBusy) return;
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
      try {
        const conversationId = await ensureConversation(query);
        setBusy(assistantId, conversationId);
        await sendChatQuery(
          query,
          assistantId,
          history,
          scopes.map((d) => d.id),
          conversationId,
        );
        void queryClient.invalidateQueries({ queryKey: ["conversations"] });
      } catch (e) {
        const msg = String(e);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant" ? { ...m, error: msg } : m,
          ),
        );
        clearBusyIf(assistantId);
        setRetryUserText(query);
      }
    },
    [
      ready,
      anyBusy,
      messages,
      scopeDocs,
      ensureConversation,
      setBusy,
      clearBusyIf,
      queryClient,
    ],
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
        : anyBusy
          ? otherChatBusy
            ? "Wait for the other chat to finish"
            : "Ask about your library"
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
            Cognito needs checking models, downloaded once.
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
            <Button type="button" variant="quiet" onClick={startNewChat}>
              New chat
            </Button>
          ) : null}
        </div>
        <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
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
                busyMessageId === assistant.id &&
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
                    <>
                      <AssistantMessage
                        message={{ ...assistant, role: "assistant" }}
                        query={user.text}
                        onCite={(c) => void openCitation(c, assistant.id)}
                        onAskEverywhere={() => void sendWithText(user.text, [])}
                        onSearchIn={(s) => {
                          const scope = [{ id: s.documentId, title: s.title }];
                          setScopeDocs(scope);
                          void sendWithText(user.text, scope);
                        }}
                      />
                      {assistant.sourceChanged ? (
                        <p className="text-muted">
                          This source changed when the index was rebuilt.
                        </p>
                      ) : null}
                    </>
                  ) : null}
                </article>
              );
            })}
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
            disabled={!ready || anyBusy}
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
