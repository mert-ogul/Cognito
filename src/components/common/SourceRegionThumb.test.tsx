import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SourceQuoteOrThumb } from "./SourceRegionThumb";
import { renderPageRegion, TINY_PNG_BASE64 } from "@/lib/tauri";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    renderPageRegion: vi.fn(),
  };
});

describe("SourceQuoteOrThumb", () => {
  beforeEach(() => {
    useSidePaneStore.setState({
      open: false,
      mode: "connections",
      passage: null,
      highlightNodeIds: [],
    });
    useCitedRegionsStore.setState({ byDoc: {} });
    vi.mocked(renderPageRegion).mockResolvedValue({
      pngBase64: TINY_PNG_BASE64,
      width: 40,
      height: 20,
    });
  });

  it("shows an image for a source with regions and opens the page on click", async () => {
    const region = { page: 2, x0: 10, y0: 20, x1: 100, y1: 80 };
    render(
      <SourceQuoteOrThumb
        path="/vault/a.pdf"
        regions={[region]}
        quote="cited quote"
        documentId="d1"
        title="Alpha"
        pageCount={5}
      />,
    );
    await waitFor(() => {
      expect(renderPageRegion).toHaveBeenCalledWith("d1", 2, {
        x0: 10,
        y0: 20,
        x1: 100,
        y1: 80,
      });
    });
    const img = await screen.findByRole("img", { name: "Source region" });
    expect(img.getAttribute("src")).toContain(TINY_PNG_BASE64);
    fireEvent.click(screen.getByRole("button", { name: "Open source region" }));
    expect(useSidePaneStore.getState().open).toBe(true);
    expect(useSidePaneStore.getState().mode).toBe("passage");
    expect(useSidePaneStore.getState().passage).toMatchObject({
      documentId: "d1",
      path: "/vault/a.pdf",
      page: 2,
      pageCount: 5,
      regions: [region],
      highlight: "cited quote",
    });
    expect(useCitedRegionsStore.getState().byDoc.d1).toEqual([region]);
  });

  it("shows quote text only when there are no regions", () => {
    render(<SourceQuoteOrThumb quote="plain quote only" />);
    expect(screen.getByText("plain quote only")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open source region" })).not.toBeInTheDocument();
  });
});
