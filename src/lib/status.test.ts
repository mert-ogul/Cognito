import { describe, expect, it } from "vitest";
import { describeStatus } from "./status";
import type { AppHealth, ImportProgress } from "./tauri";

const baseHealth: AppHealth = {
  sqliteOk: true,
  sidecarReachable: true,
  ollamaModelReady: true,
  vaultConfigured: true,
  chatModel: "phi4-mini",
  embedModel: "nomic-embed-text",
  chatModelWarmed: true,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: true,
  missingModels: [],
  indexFormatUpdating: false,
};

const indexing: ImportProgress = {
  jobId: "j1",
  status: "running",
  totalFiles: 40,
  completedFiles: 12,
  failedFiles: 0,
  phase: "parse",
  path: "/vault/report.pdf",
};

describe("describeStatus", () => {
  it("returns null when all is well", () => {
    expect(describeStatus(baseHealth, null, 0)).toBeNull();
  });

  it("prioritises Ollama down over everything else", () => {
    const line = describeStatus(
      {
        ...baseHealth,
        ollamaReachable: false,
        missingModels: ["phi4-mini"],
        sidecarReachable: false,
        warmUpError: "warm failed",
      },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Ollama isn't running", tone: "danger" });
  });

  it("prioritises missing model over sidecar and indexing", () => {
    const line = describeStatus(
      {
        ...baseHealth,
        missingModels: ["nomic-embed-text"],
        sidecarReachable: false,
        warmUpError: "warm failed",
      },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Missing model: nomic-embed-text", tone: "danger" });
  });

  it("prioritises sidecar stopped over warm-up error", () => {
    const line = describeStatus(
      {
        ...baseHealth,
        sidecarReachable: false,
        warmUpError: "warm failed",
      },
      indexing,
      5,
    );
    expect(line).toEqual({
      text: "Cognito's indexer stopped. It restarts on the next action.",
      tone: "danger",
    });
  });

  it("prioritises warm-up error over indexing", () => {
    const line = describeStatus(
      { ...baseHealth, warmUpError: "Could not load weights" },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Could not load weights", tone: "danger" });
  });

  it("prioritises index format update over indexing progress", () => {
    const line = describeStatus(
      { ...baseHealth, indexFormatUpdating: true, warmUpPending: true },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Updating the search index", tone: "muted" });
  });

  it("prioritises indexing over model loading and graph queue", () => {
    const line = describeStatus(
      { ...baseHealth, warmUpPending: true },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Indexing 12 of 40", tone: "muted" });
  });

  it("prioritises model loading over graph queue", () => {
    const line = describeStatus(
      { ...baseHealth, warmUpPending: true },
      null,
      5,
    );
    expect(line).toEqual({ text: "Loading phi4-mini", tone: "muted" });
  });

  it("reports graph queue when nothing else is wrong", () => {
    expect(describeStatus(baseHealth, null, 5)).toEqual({
      text: "Building graph, 5 files left",
      tone: "muted",
    });
    expect(describeStatus(baseHealth, null, 1)).toEqual({
      text: "Building graph, 1 file left",
      tone: "muted",
    });
  });
});
