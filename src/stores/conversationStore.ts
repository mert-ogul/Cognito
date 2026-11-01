import { create } from "zustand";

type ConversationState = {
  activeId: string | null;
  busyMessageId: string | null;
  busyConversationId: string | null;
  setActiveId: (id: string | null) => void;
  setBusy: (messageId: string | null, conversationId: string | null) => void;
  clearBusyIf: (messageId: string) => void;
  resetActive: () => void;
};

export const useConversationStore = create<ConversationState>((set, get) => ({
  activeId: null,
  busyMessageId: null,
  busyConversationId: null,
  setActiveId: (id) => set({ activeId: id }),
  setBusy: (messageId, conversationId) =>
    set({ busyMessageId: messageId, busyConversationId: conversationId }),
  clearBusyIf: (messageId) => {
    if (get().busyMessageId === messageId) {
      set({ busyMessageId: null, busyConversationId: null });
    }
  },
  resetActive: () =>
    set({
      activeId: null,
      busyMessageId: null,
      busyConversationId: null,
    }),
}));
