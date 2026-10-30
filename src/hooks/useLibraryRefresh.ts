import { useTauriEvent } from "@/lib/tauri-events";
import type { BackgroundProgress } from "@/lib/tauri";
import { useQueryClient } from "@tanstack/react-query";

/** Invalidates library queries when import/watcher/background events change the store. */
export function useLibraryRefresh() {
  const qc = useQueryClient();

  useTauriEvent("library-changed", () => {
    void qc.invalidateQueries({ queryKey: ["documents"] });
  });

  useTauriEvent<BackgroundProgress>("background-progress", (ev) => {
    void qc.invalidateQueries({ queryKey: ["documents"] });
    if (ev.payload.phase === "ok" || ev.payload.phase === "failed") {
      void qc.invalidateQueries({ queryKey: ["document-card"] });
      void qc.invalidateQueries({ queryKey: ["suggested-questions"] });
    }
  });
}
