import { create } from "zustand";

type GraphSessionState = {
  highlightNodeIds: string[];
  setHighlightNodeIds: (ids: string[]) => void;
};

export const useGraphSessionStore = create<GraphSessionState>((set) => ({
  highlightNodeIds: [],
  setHighlightNodeIds: (ids) => set({ highlightNodeIds: ids }),
}));
