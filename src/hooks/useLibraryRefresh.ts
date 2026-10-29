import { useTauriEvent } from "@/lib/tauri-events";
import type { GraphExtractionProgress } from "@/lib/tauri";
import { useQueryClient } from "@tanstack/react-query";

/** Invalidates library queries when import/watcher/graph events change the store. */
export function useLibraryRefresh() {
  const qc = useQueryClient();

  useTauriEvent("library-changed", () => {
    void qc.invalidateQueries({ queryKey: ["documents"] });
    void qc.invalidateQueries({ queryKey: ["graph-snapshot"] });
  });

  useTauriEvent<GraphExtractionProgress>("graph-extraction-progress", (ev) => {
    if (ev.payload.phase === "ok" || ev.payload.phase === "failed") {
      void qc.invalidateQueries({ queryKey: ["graph-snapshot"] });
    }
  });
}
