import { useTauriEvent } from "@/lib/tauri-events";
import type { BackgroundProgress } from "@/lib/tauri";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";

function statusPhase(phase: string, detail: string | null | undefined): boolean {
  if (phase === "ok" || phase === "failed" || phase === "pending") return true;
  if ((phase === "entities" || phase === "summarising") && !detail) return true;
  return false;
}

/** Invalidates library queries when import/watcher/background events change the store. */
export function useLibraryRefresh() {
  const qc = useQueryClient();
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingRef = useRef(false);

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, []);

  const scheduleDocumentsInvalidate = () => {
    pendingRef.current = true;
    if (timerRef.current != null) return;
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      if (!pendingRef.current) return;
      pendingRef.current = false;
      void qc.invalidateQueries({ queryKey: ["documents"] });
    }, 1000);
  };

  useTauriEvent("library-changed", () => {
    scheduleDocumentsInvalidate();
  });

  useTauriEvent<BackgroundProgress>("background-progress", (ev) => {
    if (!statusPhase(ev.payload.phase, ev.payload.detail)) {
      return;
    }
    scheduleDocumentsInvalidate();
    if (ev.payload.phase === "ok" || ev.payload.phase === "failed") {
      void qc.invalidateQueries({ queryKey: ["document-card"] });
      void qc.invalidateQueries({ queryKey: ["suggested-questions"] });
    }
  });
}
