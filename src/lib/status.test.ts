import { describe, expect, it } from "vitest";
import { describeStatus } from "./status";
import type { AppHealth, ImportProgress } from "./tauri";

const baseHealth: AppHealth = {
  sqliteOk: true,
  sidecarReachable: true,
  ollamaModelReady: true,
  vaultConfigured: true,
  chatModel: "qwen3.5:9b",
  embedModel: "qwen3-embedding:0.6b",
  chatModelWarmed: true,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: true,
  missingModels: [],
  indexFormatUpdating: false,
  checkingModelsReady: true,
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
        missingModels: ["qwen3.5:9b"],
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
        missingModels: ["qwen3-embedding:0.6b"],
        sidecarReachable: false,
        warmUpError: "warm failed",
      },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Missing model: qwen3-embedding:0.6b", tone: "danger" });
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

  it("prioritises indexing over model loading and entities queue", () => {
    const line = describeStatus(
      { ...baseHealth, warmUpPending: true },
      indexing,
      5,
    );
    expect(line).toEqual({ text: "Indexing 12 of 40", tone: "muted" });
  });

  it("prioritises model loading over entities queue", () => {
    const line = describeStatus(
      { ...baseHealth, warmUpPending: true },
      null,
      5,
    );
    expect(line).toEqual({ text: "Loading qwen3.5:9b", tone: "muted" });
  });

  it("reports entities queue when nothing else is wrong", () => {
    expect(describeStatus(baseHealth, null, 5)).toEqual({
      text: "Finding names, 5 files left",
      tone: "muted",
    });
    expect(describeStatus(baseHealth, null, 1)).toEqual({
      text: "Finding names, 1 file left",
      tone: "muted",
    });
  });
});
