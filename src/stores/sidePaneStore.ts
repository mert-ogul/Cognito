import { create } from "zustand";

export type SidePaneMode = "passage" | "graph";

export type PassageView = {
  documentId: string;
  title: string;
  pageLabel: string | null;
  text: string | null;
  highlight: string | null;
};

const OPEN_KEY = "cognito.graphPaneOpen";

function readOpen(): boolean {
  if (typeof window !== "undefined" && window.innerWidth < 1100) {
    return false;
  }
  try {
    const raw = localStorage.getItem(OPEN_KEY);
    if (raw === null) {
      return typeof window === "undefined" || window.innerWidth >= 1100;
    }
    return raw === "1";
  } catch {
    return typeof window === "undefined" || window.innerWidth >= 1100;
  }
}

function writeOpen(open: boolean) {
  try {
    localStorage.setItem(OPEN_KEY, open ? "1" : "0");
  } catch {
    /* ignore */
  }
}

type SidePaneState = {
  open: boolean;
  mode: SidePaneMode;
  passage: PassageView | null;
  highlightNodeIds: string[];
  setOpen: (open: boolean) => void;
  toggleOpen: () => void;
  setMode: (mode: SidePaneMode) => void;
  openPassage: (passage: PassageView) => void;
  openGraphWithHighlights: (ids: string[]) => void;
  setHighlightNodeIds: (ids: string[]) => void;
};

export const useSidePaneStore = create<SidePaneState>((set) => ({
  open: typeof window === "undefined" ? true : readOpen(),
  mode: "graph",
  passage: null,
  highlightNodeIds: [],
  setOpen: (open) => {
    writeOpen(open);
    set({ open });
  },
  toggleOpen: () =>
    set((s) => {
      const open = !s.open;
      writeOpen(open);
      return { open, mode: open ? "graph" : s.mode };
    }),
  setMode: (mode) => set({ mode }),
  openPassage: (passage) => {
    writeOpen(true);
    set({ open: true, mode: "passage", passage });
  },
  openGraphWithHighlights: (ids) => {
    writeOpen(true);
    set({ open: true, mode: "graph", highlightNodeIds: ids });
  },
  setHighlightNodeIds: (ids) => set({ highlightNodeIds: ids }),
}));
