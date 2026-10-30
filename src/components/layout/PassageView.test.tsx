import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PassageView } from "./PassageView";
import { openDocument, renderDocumentPage, TINY_PNG_BASE64 } from "@/lib/tauri";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    renderDocumentPage: vi.fn(),
    openDocument: vi.fn(async () => {}),
  };
});

describe("PassageView", () => {
  beforeEach(() => {
    vi.mocked(renderDocumentPage).mockResolvedValue({
      pngBase64: TINY_PNG_BASE64,
      width: 200,
      height: 280,
    });
  });

  it("navigates previous and next page within pageCount", async () => {
    render(
      <PassageView
        passage={{
          documentId: "d1",
          title: "Alpha",
          pageLabel: "p. 2",
          text: "body text",
          highlight: "body text",
          path: "/vault/a.pdf",
          page: 2,
          pageCount: 3,
          regions: [{ page: 2, x0: 10, y0: 10, x1: 50, y1: 40 }],
        }}
      />,
    );
    await waitFor(() => {
      expect(renderDocumentPage).toHaveBeenCalledWith("/vault/a.pdf", 2, 2);
    });
    expect(screen.getByText("Page 2 of 3")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => {
      expect(renderDocumentPage).toHaveBeenCalledWith("/vault/a.pdf", 3, 2);
    });
    expect(screen.getByText("Page 3 of 3")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next page" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Previous page" }));
    await waitFor(() => {
      expect(screen.getByText("Page 2 of 3")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Previous page" }));
    await waitFor(() => {
      expect(screen.getByText("Page 1 of 3")).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled();
  });

  it("keeps text-only passage when regions are absent", () => {
    render(
      <PassageView
        passage={{
          documentId: "d1",
          title: "Alpha",
          pageLabel: "p. 1",
          text: "prev\n\ncited body\n\nnext",
          highlight: "cited body",
        }}
      />,
    );
    expect(screen.getByText("cited body").className).toMatch(/bg-mark/);
    expect(screen.queryByRole("button", { name: "Next page" })).not.toBeInTheDocument();
    expect(openDocument).not.toHaveBeenCalled();
  });
});
