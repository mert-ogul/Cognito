import { GraphPanel } from "@/components/graph/GraphPanel";
import { useAppHealth } from "@/hooks/useAppHealth";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/hooks/useAppHealth", () => ({
  useAppHealth: vi.fn(),
}));

describe("GraphPanel", () => {
  it("shows unavailable message when DuckDB is not ready", () => {
    vi.mocked(useAppHealth).mockReturnValue({ data: { duckdbOk: false } } as never);
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <GraphPanel />
      </QueryClientProvider>,
    );
    expect(screen.getByText(/Graph store is not available/)).toBeInTheDocument();
  });
});
