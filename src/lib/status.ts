import type { AppHealth, ImportProgress } from "@/lib/tauri";

export type StatusTone = "danger" | "muted";

export type StatusLine = {
  text: string;
  tone: StatusTone;
};

/** Single most important status line for the toolbar, or null when all is well. */
export function describeStatus(
  health: AppHealth | undefined,
  importProgress: ImportProgress | null | undefined,
  graphQueue: number,
): StatusLine | null {
  if (!health) {
    return null;
  }
  if (!health.ollamaReachable) {
    return { text: "Ollama isn't running", tone: "danger" };
  }
  if (health.missingModels.length > 0) {
    return { text: `Missing model: ${health.missingModels[0]}`, tone: "danger" };
  }
  if (!health.sidecarReachable) {
    return {
      text: "Cognito's indexer stopped. It restarts on the next action.",
      tone: "danger",
    };
  }
  if (health.warmUpError) {
    return { text: health.warmUpError, tone: "danger" };
  }
  if (
    importProgress &&
    importProgress.totalFiles > 0 &&
    importProgress.status !== "completed" &&
    importProgress.status !== "failed" &&
    importProgress.phase !== "done"
  ) {
    return {
      text: `Indexing ${importProgress.completedFiles} of ${importProgress.totalFiles}`,
      tone: "muted",
    };
  }
  if (health.warmUpPending) {
    return { text: `Loading ${health.chatModel}`, tone: "muted" };
  }
  if (graphQueue > 0) {
    return {
      text: `Building graph, ${graphQueue} file${graphQueue === 1 ? "" : "s"} left`,
      tone: "muted",
    };
  }
  return null;
}
