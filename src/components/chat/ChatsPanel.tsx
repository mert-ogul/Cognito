import { Button } from "@/components/ui/button";
import {
  deleteConversation,
  listConversations,
  type ConversationListItem,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useConversationStore } from "@/stores/conversationStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

function formatChatDate(iso: string, now = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const startOfThat = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const dayMs = 24 * 60 * 60 * 1000;
  const diffDays = Math.round((startOfToday.getTime() - startOfThat.getTime()) / dayMs);
  if (diffDays === 0) {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false });
  }
  if (diffDays === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

type Props = {
  onOpenChat: (id: string) => void;
  onNewChat: () => void;
};

export function ChatsPanel({ onOpenChat, onNewChat }: Props) {
  const qc = useQueryClient();
  const activeId = useConversationStore((s) => s.activeId);
  const busyConversationId = useConversationStore((s) => s.busyConversationId);
  const showConversation = useMainViewStore((s) => s.showConversation);
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const list = useQuery({
    queryKey: ["conversations"],
    queryFn: listConversations,
  });

  const remove = useMutation({
    mutationFn: (id: string) => deleteConversation(id),
    onSuccess: (_void, id) => {
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      setConfirmId(null);
      if (useConversationStore.getState().activeId === id) {
        onNewChat();
      }
    },
  });

  const rows: ConversationListItem[] = list.data ?? [];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-line px-3 py-2">
        <p className="font-medium text-text">Chats</p>
        <Button type="button" variant="quiet" onClick={onNewChat}>
          New chat
        </Button>
      </div>
      <ul className="min-h-0 flex-1 overflow-auto">
        {rows.map((c) => {
          const selected = activeId === c.id;
          const busy = busyConversationId === c.id;
          const confirming = confirmId === c.id;
          return (
            <li
              key={c.id}
              className={cn(
                "group border-b border-line px-3 py-2 hover:bg-hover",
                selected ? "bg-hover" : "",
              )}
            >
              {confirming ? (
                <div className="space-y-2">
                  <p className="text-text">Delete this chat?</p>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      variant="primary"
                      disabled={remove.isPending}
                      onClick={() => remove.mutate(c.id)}
                    >
                      Delete
                    </Button>
                    <Button
                      type="button"
                      variant="secondary"
                      disabled={remove.isPending}
                      onClick={() => setConfirmId(null)}
                    >
                      Cancel
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="flex items-start gap-2">
                  <button
                    type="button"
                    className="min-w-0 flex-1 text-left"
                    onClick={() => {
                      onOpenChat(c.id);
                      showConversation();
                    }}
                  >
                    <p className="truncate text-text" title={c.title}>
                      {c.title}
                    </p>
                    <p className="text-muted">{formatChatDate(c.updatedAt)}</p>
                  </button>
                  <Button
                    type="button"
                    variant="quiet"
                    size="sm"
                    className="h-auto shrink-0 px-1 py-0 opacity-0 focus-visible:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100"
                    disabled={busy || remove.isPending}
                    onClick={() => setConfirmId(c.id)}
                  >
                    Delete
                  </Button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export { formatChatDate };
