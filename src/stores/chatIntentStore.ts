import { create } from "zustand";

export type ScopeDoc = { id: string; title: string };

export type RelatedAsk = {
  entityIdA: string;
  entityIdB: string;
  nameA: string;
  nameB: string;
};

type ChatIntentState = {
  pendingScope: ScopeDoc[];
  pendingQuestion: string | null;
  pendingRelated: RelatedAsk | null;
  setAskAbout: (scope: ScopeDoc[], question?: string) => void;
  setRelatedAsk: (related: RelatedAsk) => void;
  consumeIntent: () => { scope: ScopeDoc[]; question: string | null };
  consumeRelated: () => RelatedAsk | null;
};

export const useChatIntentStore = create<ChatIntentState>((set, get) => ({
  pendingScope: [],
  pendingQuestion: null,
  pendingRelated: null,
  setAskAbout: (scope, question) =>
    set({ pendingScope: scope, pendingQuestion: question ?? null }),
  setRelatedAsk: (related) => set({ pendingRelated: related }),
  consumeIntent: () => {
    const { pendingScope, pendingQuestion } = get();
    set({ pendingScope: [], pendingQuestion: null });
    return { scope: pendingScope, question: pendingQuestion };
  },
  consumeRelated: () => {
    const { pendingRelated } = get();
    set({ pendingRelated: null });
    return pendingRelated;
  },
}));
