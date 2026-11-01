import { invoke } from "@tauri-apps/api/core";
import {
  E2E_DOCUMENTS,
  E2E_DOCUMENT_CARD,
  E2E_HEALTH,
  E2E_SUGGESTED_QUESTIONS,
  e2eAppendTurn,
  e2eClearConversations,
  e2eCreateConversation,
  e2eDeleteConversation,
  e2eFinishAssistant,
  e2eGetConversation,
  e2eListConversations,
} from "@/lib/e2e-fixtures";
import { e2eEmit } from "@/lib/tauri-events";

const e2e = import.meta.env.VITE_E2E === true || import.meta.env.VITE_E2E === "true";

export type AppHealth = {
  sqliteOk: boolean;
  sidecarReachable: boolean;
  ollamaModelReady: boolean;
  vaultConfigured: boolean;
  chatModel: string;
  embedModel: string;
  chatModelWarmed: boolean;
  warmUpPending: boolean;
  warmUpError: string | null;
  ollamaReachable: boolean;
  missingModels: string[];
  indexFormatUpdating: boolean;
  checkingModelsReady: boolean;
};

export type DocumentRow = {
  id: string;
  path: string;
  title: string;
  fileType: string;
  contentHash: string;
  pageCount: number | null;
  status: string;
  errorMessage: string | null;
  importedAt: string;
  indexedAt: string | null;
  modifiedAt: string;
  entitiesStatus: string;
  entitiesError: string | null;
  cardStatus: string;
  cardType: string | null;
  cardDate: string | null;
  cardPeople: string[];
  cardOrganizations: string[];
  scanStatus?: string | null;
};

export type ImportProgress = {
  jobId: string;
  status: string;
  totalFiles: number;
  completedFiles: number;
  failedFiles: number;
  phase: string;
  path?: string | null;
  errorMessage?: string | null;
};

export type BackgroundProgress = {
  jobId?: string | null;
  documentId: string;
  phase: string;
  detail?: string | null;
};


export type DocumentCardPayload = {
  documentId: string;
  type: string;
  title: string;
  date: string | null;
  people: unknown[];
  organizations: string[];
  summary: unknown[];
  facts: unknown[];
  fields: unknown[];
  aliases: string[];
  questions: string[];
  model: string;
  createdAt: string;
};

export type SuggestedQuestion = {
  question: string;
  scopeIds: string[];
  label?: string | null;
};

export type SettingsPayload = {
  vaultPath: string | null;
  chatModel: string;
  embedModel: string;
  backgroundModel: string;
  summariseDocuments: boolean;
  extractEntities: boolean;
  extraEntityLabels: string;
};

export async function getAppHealth(): Promise<AppHealth> {
  if (e2e) {
    return { ...(E2E_HEALTH as unknown as AppHealth) };
  }
  return invoke("get_app_health");
}

export async function warmChatModel(): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("warm_chat_model");
}

export async function openLogsFolder(): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("open_logs_folder");
}

export async function selectVaultFolder(): Promise<string | null> {
  if (e2e) {
    return "/fixture";
  }
  return invoke("select_vault_folder");
}

export async function setVault(path: string): Promise<string | null> {
  if (e2e) {
    void path;
    return "e2e-set-vault";
  }
  return invoke("set_vault", { path });
}

export async function clearLibrary(): Promise<void> {
  if (e2e) {
    e2eClearConversations();
    return;
  }
  return invoke("clear_library");
}

export type ConversationListItem = {
  id: string;
  title: string;
  updatedAt: string;
};

export type StoredChatMessage = {
  id: string;
  conversationId: string;
  position: number;
  role: string;
  status: string;
  content: string;
  payload: Record<string, unknown> | null;
  createdAt: string;
};

export type ConversationDetail = {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messages: StoredChatMessage[];
};

export async function listConversations(): Promise<ConversationListItem[]> {
  if (e2e) {
    return e2eListConversations();
  }
  return invoke("list_conversations");
}

export async function getConversation(
  id: string,
): Promise<ConversationDetail | null> {
  if (e2e) {
    const c = e2eGetConversation(id);
    if (!c) return null;
    return {
      id: c.id,
      title: c.title,
      createdAt: c.createdAt,
      updatedAt: c.updatedAt,
      messages: c.messages.map((m) => ({
        id: m.id,
        conversationId: m.conversationId,
        position: m.position,
        role: m.role,
        status: m.status,
        content: m.content,
        payload: m.payload,
        createdAt: m.createdAt,
      })),
    };
  }
  return invoke("get_conversation", { id });
}

export async function createConversation(
  title: string,
): Promise<ConversationListItem> {
  if (e2e) {
    return e2eCreateConversation(title);
  }
  return invoke("create_conversation", { title });
}

export async function deleteConversation(id: string): Promise<void> {
  if (e2e) {
    e2eDeleteConversation(id);
    return;
  }
  return invoke("delete_conversation", { id });
}

export async function listDocuments(): Promise<DocumentRow[]> {
  if (e2e) {
    return [...E2E_DOCUMENTS] as unknown as DocumentRow[];
  }
  return invoke("list_documents");
}

export async function importDocuments(folder?: string | null): Promise<string> {
  if (e2e) {
    return "e2e-job";
  }
  return invoke("import_documents", { folder: folder ?? null });
}

export async function retryFailedImport(): Promise<string> {
  if (e2e) {
    return "e2e-retry";
  }
  return invoke("retry_failed_import");
}

export type PageRegion = {
  page: number;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
};

export type CitationItem = {
  documentId: string;
  title: string;
  path: string;
  pageStart: number | null;
  pageEnd: number | null;
  chunkId: string;
  snippet: string;
  nodeIds: string[];
  index: number;
  regions?: PageRegion[] | null;
};

export type DocumentPreview = {
  documentId: string;
  text: string | null;
  highlightSnippet: string | null;
  pageStart: number | null;
  pageEnd: number | null;
  regions?: PageRegion[] | null;
  path?: string;
  scanStatus?: string | null;
};

export type PageRender = {
  pngBase64: string;
  width: number;
  height: number;
};

/** 1×1 PNG used by E2E stubs and unit-test defaults. */
export const TINY_PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

export type AnswerSentence = {
  text: string;
  status: string; // supported | unsupported | unchecked
  sources: number[];
  reason?: string | null;
};

export type ModelsStatus = {
  ready: boolean;
  models: { name: string; present: boolean; sizeBytes: number }[];
  totalSizeBytes: number;
};

export type ChatTurn = {
  role: "user" | "assistant" | "system";
  content: string;
};

export async function getDocumentPreview(
  documentId: string,
  chunkId?: string | null,
): Promise<DocumentPreview> {
  if (e2e) {
    const t = "Intro\n\nfixture evidence text\n\noutro";
    return {
      documentId,
      text: t,
      highlightSnippet: "fixture evidence text",
      pageStart: null,
      pageEnd: null,
      regions: null,
      path: undefined,
      scanStatus: null,
    };
  }
  return invoke("get_document_preview", {
    documentId,
    chunkId: chunkId ?? null,
  });
}

export async function renderPageRegion(
  documentId: string,
  page: number,
  rect: { x0: number; y0: number; x1: number; y1: number },
  scale?: number,
): Promise<PageRender> {
  if (e2e) {
    void documentId;
    void page;
    void rect;
    void scale;
    return { pngBase64: TINY_PNG_BASE64, width: 1, height: 1 };
  }
  return invoke("render_page_region", {
    documentId,
    page,
    rect,
    scale: scale ?? null,
  });
}

export async function renderDocumentPage(
  documentId: string,
  page: number,
  scale?: number,
): Promise<PageRender> {
  if (e2e) {
    void documentId;
    void page;
    void scale;
    return { pngBase64: TINY_PNG_BASE64, width: 1, height: 1 };
  }
  return invoke("render_document_page", {
    documentId,
    page,
    scale: scale ?? null,
  });
}

export async function openDocument(documentId: string): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("open_document", { documentId });
}

export async function getDocumentCard(documentId: string): Promise<DocumentCardPayload | null> {
  if (e2e) {
    if (documentId === E2E_DOCUMENT_CARD.documentId) {
      return { ...(E2E_DOCUMENT_CARD as unknown as DocumentCardPayload) };
    }
    return null;
  }
  return invoke("get_document_card", { documentId });
}

export async function getSuggestedQuestions(): Promise<SuggestedQuestion[]> {
  if (e2e) {
    return [...E2E_SUGGESTED_QUESTIONS] as unknown as SuggestedQuestion[];
  }
  return invoke("get_suggested_questions");
}

export async function sendChatQuery(
  query: string,
  messageId: string,
  history: ChatTurn[] = [],
  scopeIds: string[] = [],
  conversationId = "",
): Promise<string> {
  if (e2e) {
    e2eAppendTurn(conversationId, query, messageId);
    const citations = [
      {
        documentId: "e2e-doc-1",
        title: "Fixture sample",
        path: "/fixture/sample.md",
        pageStart: null,
        pageEnd: null,
        chunkId: "c1",
        snippet: "fixture evidence text",
        nodeIds: ["n-e2e-1"],
        index: 1,
      },
    ];
    const sentences = [
      {
        text: "Hello world.",
        status: "supported",
        sources: [1],
      },
    ];
    window.setTimeout(() => {
      e2eEmit("chat-stage", {
        messageId,
        conversationId,
        stage: "passages",
        citations,
      });
      e2eFinishAssistant(messageId, "Hello world.", {
        citations,
        sentences,
        notFound: false,
        scopeTitle: null,
        suggestions: [],
        error: null,
      });
      e2eEmit("chat-final", {
        messageId,
        conversationId,
        citations,
        sentences,
        notFound: false,
        retrievalMode: "hybrid",
        error: null,
        scopeTitle: null,
        scopeIds,
      });
    }, 30);
    return messageId;
  }
  return invoke<string>("send_chat_query", {
    query,
    messageId,
    conversationId,
    history,
    scopeIds,
  });
}

export async function prepareModels(): Promise<ModelsStatus> {
  if (e2e) {
    return { ready: true, models: [], totalSizeBytes: 0 };
  }
  return invoke("prepare_models");
}

export async function getModelsStatus(): Promise<ModelsStatus> {
  if (e2e) {
    return { ready: true, models: [], totalSizeBytes: 0 };
  }
  return invoke("get_models_status");
}


export async function getSettings(): Promise<SettingsPayload> {
  if (e2e) {
    return {
      vaultPath: "/fixture",
      chatModel: "qwen3.5:9b",
      embedModel: "qwen3-embedding:0.6b",
      backgroundModel: "qwen3.5:9b",
      summariseDocuments: true,
      extractEntities: true,
      extraEntityLabels: "",
    };
  }
  return invoke("get_settings");
}

export async function updateSettings(payload: {
  vaultPath?: string | null;
  chatModel: string;
  embedModel: string;
  backgroundModel: string;
  summariseDocuments: boolean;
  extractEntities: boolean;
  extraEntityLabels: string;
}): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("update_settings", { payload });
}


export type EntityIndexItem = {
  id: string;
  name: string;
  normalized: string;
  label: string;
  mentionCount: number;
  docCount: number;
};

export type EntityPassage = {
  chunkId: string;
  snippet: string;
  pageStart: number | null;
  pageEnd: number | null;
};

export type EntityDocument = {
  documentId: string;
  title: string;
  path: string;
  passages: EntityPassage[];
};

export type EntityCoMention = {
  entityId: string;
  name: string;
  label: string;
  count: number;
};

export type EntityDetail = {
  id: string;
  name: string;
  label: string;
  normalized: string;
  docCount: number;
  mentionCount: number;
  aliases: string[];
  documents: EntityDocument[];
  coMentions: EntityCoMention[];
};

export type TimelineItem = {
  kind: "document" | "entity";
  id: string;
  title?: string;
  name?: string;
  date: string;
  documentId?: string;
  entityId?: string;
};

export type ConnectionsTimeline = {
  items: TimelineItem[];
};

export type RelatedDocument = {
  id: string;
  title: string;
  kind: "shares_names" | "mentions" | "similar" | string;
  kindLabel: string;
  sharedEntities: string[];
};

export async function rebuildConnections(): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("rebuild_connections");
}

export async function getEntitiesIndex(): Promise<EntityIndexItem[]> {
  if (e2e) {
    return [];
  }
  return invoke("get_entities_index");
}

export async function getEntityDetail(entityId: string): Promise<EntityDetail | null> {
  if (e2e) {
    void entityId;
    return null;
  }
  return invoke("get_entity_detail", { entityId });
}

export async function getRelatedDocuments(documentId: string): Promise<RelatedDocument[]> {
  if (e2e) {
    void documentId;
    return [];
  }
  return invoke("get_related_documents", { documentId });
}

export async function getConnectionsTimeline(): Promise<ConnectionsTimeline> {
  if (e2e) {
    return { items: [] };
  }
  return invoke("get_connections_timeline");
}

export async function sendRelatedQuery(
  entityIdA: string,
  entityIdB: string,
  messageId: string,
  conversationId = "",
): Promise<string> {
  if (e2e) {
    e2eAppendTurn(
      conversationId,
      `How are ${entityIdA} and ${entityIdB} related?`,
      messageId,
    );
    window.setTimeout(() => {
      e2eEmit("chat-stage", {
        messageId,
        conversationId,
        stage: "passages",
        citations: [],
      });
      e2eFinishAssistant(messageId, "", {
        citations: [],
        sentences: [],
        notFound: true,
        scopeTitle: null,
        suggestions: [],
        error: null,
      });
      e2eEmit("chat-final", {
        messageId,
        conversationId,
        citations: [],
        sentences: [],
        notFound: true,
        retrievalMode: "related",
        error: null,
        scopeTitle: null,
        scopeIds: [],
      });
    }, 30);
    return messageId;
  }
  return invoke<string>("send_related_query", {
    entityIdA,
    entityIdB,
    messageId,
    conversationId,
  });
}

export async function rebuildIndex(embedModel?: string): Promise<string> {
  if (e2e) {
    return "e2e-rebuild-job";
  }
  return invoke<string>("rebuild_index", {
    embedModel: embedModel ?? null,
  });
}
