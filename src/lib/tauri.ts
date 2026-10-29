import { invoke } from "@tauri-apps/api/core";
import { E2E_DOCUMENTS, E2E_GRAPH_SNAPSHOT, E2E_HEALTH, E2E_NODE_DETAIL } from "@/lib/e2e-fixtures";
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
  graphStatus: string;
  graphError: string | null;
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

export type GraphExtractionProgress = {
  jobId?: string | null;
  documentId: string;
  phase: string;
  detail?: string | null;
};

export type GraphSnapshotNode = {
  nodeId: string;
  name: string;
  kind: string;
  normalizedName: string;
  sourceCount: number;
  confidenceAvg: number;
};

export type GraphSnapshotEdge = {
  edgeId: string;
  sourceNodeId: string;
  targetNodeId: string;
  relation: string;
  documentId: string;
  chunkId: string;
  confidence: number;
};

export type GraphSnapshot = {
  nodes: GraphSnapshotNode[];
  edges: GraphSnapshotEdge[];
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

export async function pingSidecar(): Promise<Record<string, unknown>> {
  if (e2e) {
    return { ok: true };
  }
  return invoke("ping_sidecar");
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
    return;
  }
  return invoke("clear_library");
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
};

export type ChatTurn = {
  role: "user" | "assistant" | "system";
  content: string;
};

export async function getDocumentPreview(
  documentId: string,
  chunkId?: string | null,
): Promise<{
  documentId: string;
  text: string | null;
  highlightSnippet: string | null;
  pageStart: number | null;
  pageEnd: number | null;
}> {
  if (e2e) {
    const t = "Intro\n\nfixture evidence text\n\noutro";
    return {
      documentId,
      text: t,
      highlightSnippet: "fixture evidence text",
      pageStart: null,
      pageEnd: null,
    };
  }
  return invoke("get_document_preview", {
    documentId,
    chunkId: chunkId ?? null,
  });
}

export async function openDocument(documentId: string): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("open_document", { documentId });
}

export async function sendChatQuery(
  query: string,
  messageId: string,
  history: ChatTurn[] = [],
): Promise<string> {
  if (e2e) {
    window.setTimeout(() => {
      e2eEmit("chat-stream", { messageId, token: "Hello world [1]." });
      e2eEmit("chat-final", {
        messageId,
        citations: [
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
        ],
        retrievalMode: "hybrid",
        error: null,
      });
    }, 30);
    return messageId;
  }
  return invoke<string>("send_chat_query", { query, messageId, history });
}

export async function getGraphSnapshot(limitNodes?: number | null): Promise<GraphSnapshot> {
  if (e2e) {
    void limitNodes;
    return { ...(E2E_GRAPH_SNAPSHOT as unknown as GraphSnapshot) };
  }
  return invoke("get_graph_snapshot", { limitNodes: limitNodes ?? null });
}

export async function searchGraphNodes(query: string, limit?: number | null): Promise<unknown[]> {
  if (e2e) {
    const q = query.toLowerCase();
    const cap = limit ?? 20;
    return E2E_GRAPH_SNAPSHOT.nodes
      .filter((n) => n.name.toLowerCase().includes(q))
      .slice(0, cap) as unknown[];
  }
  return invoke("search_graph", { query, limit: limit ?? null });
}

export async function getGraphNodeDetail(nodeId: string): Promise<unknown | null> {
  if (e2e) {
    if (nodeId === "n-e2e-1") {
      return { ...(E2E_NODE_DETAIL as unknown as Record<string, unknown>) };
    }
    if (nodeId === "n-e2e-2") {
      return {
        node: {
          nodeId: "n-e2e-2",
          name: "Related Term",
          kind: "topic",
          normalizedName: "related term",
          sourceCount: 1,
          confidenceAvg: 0.7,
          metadataJson: null,
          createdAt: "2026-01-01",
          updatedAt: "2026-01-01",
        },
        edges: E2E_GRAPH_SNAPSHOT.edges,
      };
    }
    return null;
  }
  return invoke("get_graph_node_detail", { nodeId });
}

export async function getSettings(): Promise<{
  vaultPath: string | null;
  chatModel: string;
  embedModel: string;
  graphExtractionEnabled: boolean;
}> {
  if (e2e) {
    return {
      vaultPath: "/fixture",
      chatModel: "phi4-mini",
      embedModel: "nomic-embed-text",
      graphExtractionEnabled: true,
    };
  }
  return invoke("get_settings");
}

export async function updateSettings(payload: {
  vaultPath?: string | null;
  chatModel: string;
  embedModel: string;
  graphExtractionEnabled: boolean;
}): Promise<void> {
  if (e2e) {
    return;
  }
  return invoke("update_settings", { payload });
}

export async function rebuildIndex(embedModel?: string): Promise<string> {
  if (e2e) {
    return "e2e-rebuild-job";
  }
  return invoke<string>("rebuild_index", {
    embedModel: embedModel ?? null,
  });
}
