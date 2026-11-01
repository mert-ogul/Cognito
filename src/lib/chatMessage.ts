import type {
  AnswerSentence,
  CitationItem,
  StoredChatMessage,
} from "@/lib/tauri";
import type { ScopeSearchSuggestion } from "@/components/chat/AssistantMessage";

export type ChatMessage = {
  id: string;
  role: "user" | "assistant";
  text: string;
  citations?: CitationItem[];
  sentences?: AnswerSentence[];
  draft?: string[];
  notFound?: boolean;
  partial?: boolean;
  scopeTitle?: string | null;
  suggestions?: ScopeSearchSuggestion[];
  error?: string;
  stage?: "passages" | "ranking" | "writing" | "checking" | "done";
  stageDetail?: string;
  sourceChanged?: boolean;
};

export function conversationTitle(text: string): string {
  const collapsed = text.split(/\s+/).filter(Boolean).join(" ");
  if (collapsed.length <= 60) return collapsed;
  const words = collapsed.split(" ");
  let cut = "";
  for (const word of words) {
    const next = cut ? `${cut} ${word}` : word;
    if (next.length > 60) break;
    cut = next;
  }
  if (!cut) return `${collapsed.slice(0, 60)}…`;
  return `${cut}…`;
}

export function storedMessageToChatMessage(row: StoredChatMessage): ChatMessage {
  const role = row.role === "assistant" ? "assistant" : "user";
  const payload = row.payload ?? {};
  const error =
    row.status === "error"
      ? (typeof payload.error === "string" && payload.error
          ? payload.error
          : row.content || "This answer was interrupted.")
      : undefined;
  const sentences = Array.isArray(payload.sentences)
    ? (payload.sentences as AnswerSentence[])
    : undefined;
  const citations = Array.isArray(payload.citations)
    ? (payload.citations as CitationItem[])
    : undefined;
  const suggestions = Array.isArray(payload.suggestions)
    ? (payload.suggestions as ScopeSearchSuggestion[])
    : undefined;
  return {
    id: row.id,
    role,
    text: row.content,
    citations,
    sentences,
    notFound: payload.notFound === true,
    partial: payload.partial === true,
    scopeTitle:
      typeof payload.scopeTitle === "string" || payload.scopeTitle === null
        ? (payload.scopeTitle as string | null)
        : undefined,
    suggestions,
    error,
    stage: row.status === "done" || row.status === "error" ? "done" : undefined,
  };
}
