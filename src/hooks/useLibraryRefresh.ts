import { listen } from "@/lib/tauri-events";
import type { GraphExtractionProgress } from "@/lib/tauri";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

/** Invalidates library queries when import/watcher/graph events change the store. */
export function useLibraryRefresh() {
  const qc = useQueryClient();

  useEffect(() => {
    const un: Array<() => void> = [];
    let cancelled = false;
    void (async () => {
      const u1 = await listen("library-changed", () => {
        void qc.invalidateQueries({ queryKey: ["documents"] });
        void qc.invalidateQueries({ queryKey: ["graph-snapshot"] });
      });
      const u2 = await listen<GraphExtractionProgress>("graph-extraction-progress", (ev) => {
        if (ev.payload.phase === "ok" || ev.payload.phase === "failed") {
          void qc.invalidateQueries({ queryKey: ["graph-snapshot"] });
        }
      });
      if (!cancelled) {
        un.push(u1, u2);
      } else {
        u1();
        u2();
      }
    })();
    return () => {
      cancelled = true;
      un.forEach((f) => f());
    };
  }, [qc]);
}
