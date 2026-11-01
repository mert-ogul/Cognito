import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChatsPanel, formatChatDate } from "./ChatsPanel";
import {
  deleteConversation,
  getConversation,
  listConversations,
} from "@/lib/tauri";
import { useConversationStore } from "@/stores/conversationStore";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    listConversations: vi.fn(async () => []),
    getConversation: vi.fn(async () => null),
    deleteConversation: vi.fn(async () => {}),
  };
});

function renderPanel(
  props: {
    onOpenChat?: (id: string) => void;
    onNewChat?: () => void;
  } = {},
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const onOpenChat = props.onOpenChat ?? vi.fn();
  const onNewChat = props.onNewChat ?? vi.fn();
  return {
    onOpenChat,
    onNewChat,
    ...render(
      <QueryClientProvider client={client}>
        <ChatsPanel onOpenChat={onOpenChat} onNewChat={onNewChat} />
      </QueryClientProvider>,
    ),
  };
}

describe("formatChatDate", () => {
  it("formats today, yesterday, and other days", () => {
    const now = new Date("2026-10-31T15:00:00");
    expect(formatChatDate("2026-10-31T14:32:00", now)).toMatch(/14:32/);
    expect(formatChatDate("2026-10-30T10:00:00", now)).toBe("Yesterday");
    expect(formatChatDate("2026-10-12T10:00:00", now)).toMatch(/12/);
  });
});

describe("ChatsPanel", () => {
  beforeEach(() => {
    vi.mocked(listConversations).mockReset();
    vi.mocked(getConversation).mockReset();
    vi.mocked(deleteConversation).mockReset();
    vi.mocked(deleteConversation).mockResolvedValue();
    useConversationStore.setState({
      activeId: null,
      busyMessageId: null,
      busyConversationId: null,
    });
  });

  it("renders chats newest first", async () => {
    vi.mocked(listConversations).mockResolvedValue([
      { id: "c2", title: "Newer", updatedAt: "2026-10-31T12:00:00Z" },
      { id: "c1", title: "Older", updatedAt: "2026-10-30T12:00:00Z" },
    ]);
    renderPanel();
    const newer = await screen.findByText("Newer");
    const older = screen.getByText("Older");
    expect(
      newer.compareDocumentPosition(older) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("clicking a chat opens it", async () => {
    vi.mocked(listConversations).mockResolvedValue([
      { id: "c1", title: "About Ada", updatedAt: "2026-10-31T12:00:00Z" },
    ]);
    const { onOpenChat } = renderPanel();
    fireEvent.click(await screen.findByText("About Ada"));
    expect(onOpenChat).toHaveBeenCalledWith("c1");
  });

  it("delete asks first then removes the row", async () => {
    vi.mocked(listConversations)
      .mockResolvedValueOnce([
        { id: "c1", title: "Doomed", updatedAt: "2026-10-31T12:00:00Z" },
      ])
      .mockResolvedValueOnce([]);
    renderPanel();
    await screen.findByText("Doomed");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByText("Delete this chat?")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(deleteConversation).toHaveBeenCalledWith("c1"));
    await waitFor(() => expect(screen.queryByText("Doomed")).toBeNull());
  });

  it("deleting the open chat starts a new one", async () => {
    useConversationStore.setState({ activeId: "c1" });
    vi.mocked(listConversations)
      .mockResolvedValueOnce([
        { id: "c1", title: "Open one", updatedAt: "2026-10-31T12:00:00Z" },
      ])
      .mockResolvedValueOnce([]);
    const { onNewChat } = renderPanel();
    await screen.findByText("Open one");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onNewChat).toHaveBeenCalled());
  });
});
