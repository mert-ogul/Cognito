import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { OllamaSetupBanner } from "./OllamaSetupBanner";
import type { AppHealth } from "@/lib/tauri";

const base: AppHealth = {
  sqliteOk: true,
  sidecarReachable: true,
  ollamaModelReady: false,
  vaultConfigured: true,
  chatModel: "qwen3.5:9b",
  embedModel: "qwen3-embedding:0.6b",
  chatModelWarmed: false,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: false,
  missingModels: ["qwen3.5:9b", "qwen3-embedding:0.6b"],
  indexFormatUpdating: false,
  checkingModelsReady: true,
};

describe("OllamaSetupBanner", () => {
  it("shows not-running message when Ollama is unreachable", () => {
    render(<OllamaSetupBanner health={{ ...base, ollamaReachable: false }} />);
    expect(screen.getByText("Ollama isn't running")).toBeInTheDocument();
    expect(screen.getByText(/ollama serve/)).toBeInTheDocument();
  });

  it("lists pull commands for missing models when reachable", () => {
    render(
      <OllamaSetupBanner
        health={{
          ...base,
          ollamaReachable: true,
          missingModels: ["qwen3.5:9b", "qwen3-embedding:0.6b"],
        }}
      />,
    );
    expect(screen.getByText("These Ollama models are missing")).toBeInTheDocument();
    expect(screen.getByText("ollama pull qwen3.5:9b")).toBeInTheDocument();
    expect(screen.getByText("ollama pull qwen3-embedding:0.6b")).toBeInTheDocument();
  });
});
