import type { PageRegion } from "@/lib/tauri";
import { create } from "zustand";

type SidePaneMode = "passage" | "connections";

export type PassageView = {
  documentId: string;
  title: string;
  pageLabel: string | null;
  text: string | null;
  highlight: string | null;
  path?: string;
  page?: number;
  pageCount?: number | null;
  regions?: PageRegion[] | null;
};

const OPEN_KEY = "cognito.sidePaneOpen";

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
  selectedEntityId: string | null;
  setOpen: (open: boolean) => void;
  toggleOpen: () => void;
  setMode: (mode: SidePaneMode) => void;
  openPassage: (passage: PassageView) => void;
  openConnectionsWithHighlights: (ids: string[]) => void;
  setHighlightNodeIds: (ids: string[]) => void;
  setSelectedEntityId: (id: string | null) => void;
};

export const useSidePaneStore = create<SidePaneState>((set) => ({
  open: typeof window === "undefined" ? true : readOpen(),
  mode: "passage",
  passage: null,
  highlightNodeIds: [],
  selectedEntityId: null,
  setOpen: (open) => {
    writeOpen(open);
    set({ open });
  },
  toggleOpen: () =>
    set((s) => {
      const open = !s.open;
      writeOpen(open);
      return { open, mode: open ? s.mode : s.mode };
    }),
  setMode: (mode) => set({ mode }),
  openPassage: (passage) => {
    writeOpen(true);
    set({ open: true, mode: "passage", passage });
  },
  openConnectionsWithHighlights: (ids) => {
    writeOpen(true);
    set({ open: true, mode: "connections", highlightNodeIds: ids });
  },
  setHighlightNodeIds: (ids) => set({ highlightNodeIds: ids }),
  setSelectedEntityId: (id) => set({ selectedEntityId: id }),
}));
