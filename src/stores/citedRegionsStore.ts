import type { PageRegion } from "@/lib/tauri";
import { create } from "zustand";

type CitedRegionsState = {
  byDoc: Record<string, PageRegion[]>;
  addRegions: (documentId: string, regions: PageRegion[] | null | undefined) => void;
  clearDocument: (documentId: string) => void;
};

function regionKey(r: PageRegion): string {
  return `${r.page}:${r.x0}:${r.y0}:${r.x1}:${r.y1}`;
}

export const useCitedRegionsStore = create<CitedRegionsState>((set) => ({
  byDoc: {},
  addRegions: (documentId, regions) => {
    if (!regions || regions.length === 0) return;
    set((s) => {
      const prev = s.byDoc[documentId] ?? [];
      const seen = new Set(prev.map(regionKey));
      const next = [...prev];
      for (const r of regions) {
        const k = regionKey(r);
        if (seen.has(k)) continue;
        seen.add(k);
        next.push(r);
      }
      if (next.length === prev.length) return s;
      return { byDoc: { ...s.byDoc, [documentId]: next } };
    });
  },
  clearDocument: (documentId) =>
    set((s) => {
      if (!(documentId in s.byDoc)) return s;
      const { [documentId]: _removed, ...rest } = s.byDoc;
      return { byDoc: rest };
    }),
}));
