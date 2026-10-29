import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { OllamaSetupBanner } from "./OllamaSetupBanner";
import type { AppHealth } from "@/lib/tauri";

const base: AppHealth = {
  sqliteOk: true,
  sidecarReachable: true,
  ollamaModelReady: false,
  vaultConfigured: true,
  chatModel: "phi4-mini",
  embedModel: "nomic-embed-text",
  chatModelWarmed: false,
  warmUpPending: false,
  warmUpError: null,
  ollamaReachable: false,
  missingModels: ["phi4-mini", "nomic-embed-text"],
};

describe("OllamaSetupBanner", () => {
  it("shows not-running message when Ollama is unreachable", () => {
    render(<OllamaSetupBanner health={{ ...base, ollamaReachable: false }} />);
    expect(screen.getByText("Ollama is not running")).toBeInTheDocument();
    expect(screen.getByText(/ollama serve/)).toBeInTheDocument();
  });

  it("lists pull commands for missing models when reachable", () => {
    render(
      <OllamaSetupBanner
        health={{
          ...base,
          ollamaReachable: true,
          missingModels: ["phi4-mini", "nomic-embed-text"],
        }}
      />,
    );
    expect(screen.getByText("Required Ollama models are missing")).toBeInTheDocument();
    expect(screen.getByText("ollama pull phi4-mini")).toBeInTheDocument();
    expect(screen.getByText("ollama pull nomic-embed-text")).toBeInTheDocument();
  });
});
