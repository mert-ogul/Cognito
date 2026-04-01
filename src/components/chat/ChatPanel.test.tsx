import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ChatPanel } from "./ChatPanel";
import { RetrievalModeProvider } from "@/context/RetrievalModeContext";

vi.mock("@/lib/tauri-events", () => ({
  listen: vi.fn(async () => () => {}),
}));

const health = {
  sqliteOk: true,
  duckdbOk: true,
  sidecarReachable: true,
  ollamaModelReady: true,
  vaultConfigured: true,
  chatModel: "phi4-mini",
  embedModel: "nomic-embed-text",
  chatModelWarmed: true,
  warmUpPending: false,
  warmUpError: null,
};

describe("ChatPanel", () => {
  it("disables send when model not warmed", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <RetrievalModeProvider>
          <ChatPanel
            health={{
              ...health,
              chatModelWarmed: false,
              warmUpPending: true,
            }}
          />
        </RetrievalModeProvider>
      </QueryClientProvider>,
    );
    expect(screen.getByPlaceholderText(/Loading model/)).toBeInTheDocument();
  });
});
