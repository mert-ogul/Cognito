export const E2E_HEALTH = {
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
} as const;

export const E2E_DOCUMENTS = [
  {
    id: "e2e-doc-1",
    path: "/fixture/sample.md",
    title: "Fixture sample",
    fileType: "markdown",
    contentHash: "e2e",
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
    cardDate: "2026-01-01",
    cardPeople: ["Ada Chen"],
    cardOrganizations: ["Meridian Labs"],
  },
] as const;

export const E2E_DOCUMENT_CARD = {
  documentId: "e2e-doc-1",
  type: "note",
  title: "Fixture sample",
  date: "2026-01-01",
  people: [{ name: "Ada Chen", role: "author" }],
  organizations: ["Meridian Labs"],
  summary: [
    {
      text: "Ada Chen works at Meridian Labs.",
      quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
    },
  ],
  facts: [
    {
      text: "Ada Chen works at Meridian Labs.",
      quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
    },
  ],
  fields: [
    {
      key: "topic",
      value: "fixture",
      quotes: [{ source: 1, text: "Ada Chen works at Meridian Labs." }],
    },
  ],
  aliases: [],
  questions: ["What does Ada Chen do?"],
  model: "qwen3.5:4b",
  createdAt: "2026-01-01T02:00:00Z",
} as const;

export const E2E_SUGGESTED_QUESTIONS = [
  {
    question: "What does Ada Chen do?",
    scopeIds: ["e2e-doc-1"],
    label: null,
  },
] as const;

export type E2eConversationMessage = {
  id: string;
  conversationId: string;
  position: number;
  role: "user" | "assistant";
  status: "pending" | "done" | "error";
  content: string;
  payload: Record<string, unknown> | null;
  createdAt: string;
};

export type E2eConversation = {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messages: E2eConversationMessage[];
};

const e2eConversations = new Map<string, E2eConversation>();

function e2eNow(): string {
  return new Date().toISOString();
}

export function e2eListConversations(): {
  id: string;
  title: string;
  updatedAt: string;
}[] {
  return [...e2eConversations.values()]
    .map((c) => ({ id: c.id, title: c.title, updatedAt: c.updatedAt }))
    .sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : -1));
}

export function e2eGetConversation(id: string): E2eConversation | null {
  const c = e2eConversations.get(id);
  return c ? { ...c, messages: [...c.messages] } : null;
}

export function e2eCreateConversation(title: string): {
  id: string;
  title: string;
  updatedAt: string;
} {
  const id = `e2e-conv-${crypto.randomUUID()}`;
  const now = e2eNow();
  const row: E2eConversation = {
    id,
    title: title.trim() || "New chat",
    createdAt: now,
    updatedAt: now,
    messages: [],
  };
  e2eConversations.set(id, row);
  return { id, title: row.title, updatedAt: now };
}

export function e2eDeleteConversation(id: string): void {
  e2eConversations.delete(id);
}

export function e2eAppendTurn(
  conversationId: string,
  userText: string,
  assistantId: string,
): void {
  const c = e2eConversations.get(conversationId);
  if (!c) return;
  const now = e2eNow();
  const pos = c.messages.length;
  if (c.messages.length === 0) {
    c.title = userText.trim() || c.title;
  }
  c.messages.push({
    id: crypto.randomUUID(),
    conversationId,
    position: pos,
    role: "user",
    status: "done",
    content: userText,
    payload: null,
    createdAt: now,
  });
  c.messages.push({
    id: assistantId,
    conversationId,
    position: pos + 1,
    role: "assistant",
    status: "pending",
    content: "",
    payload: null,
    createdAt: now,
  });
  c.updatedAt = now;
}

export function e2eFinishAssistant(
  messageId: string,
  content: string,
  payload: Record<string, unknown>,
): void {
  for (const c of e2eConversations.values()) {
    const m = c.messages.find((x) => x.id === messageId);
    if (!m) continue;
    m.status = payload.error ? "error" : "done";
    m.content = content;
    m.payload = payload;
    c.updatedAt = e2eNow();
    return;
  }
}

export function e2eClearConversations(): void {
  e2eConversations.clear();
}


