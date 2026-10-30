import { create } from "zustand";

type MainViewState = {
  view: "conversation" | "document";
  documentId: string | null;
  setDocument: (documentId: string) => void;
  showConversation: () => void;
};

export const useMainViewStore = create<MainViewState>((set) => ({
  view: "conversation",
  documentId: null,
  setDocument: (documentId) => set({ view: "document", documentId }),
  showConversation: () => set({ view: "conversation", documentId: null }),
}));
