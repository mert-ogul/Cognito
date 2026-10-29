import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PanelErrorBoundary } from "./PanelErrorBoundary";
import { useState } from "react";

function BoomChild({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) {
    throw new Error("boom panel");
  }
  return <p>panel ok</p>;
}

function Harness() {
  const [shouldThrow, setShouldThrow] = useState(true);
  return (
    <div>
      <button type="button" onClick={() => setShouldThrow(false)}>
        heal
      </button>
      <PanelErrorBoundary>
        <BoomChild shouldThrow={shouldThrow} />
      </PanelErrorBoundary>
    </div>
  );
}

describe("PanelErrorBoundary", () => {
  it("shows fallback and Try again re-renders the child", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<Harness />);
    expect(screen.getByText("This panel hit an error")).toBeInTheDocument();
    expect(screen.getByText("boom panel")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "heal" }));
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.getByText("panel ok")).toBeInTheDocument();
    spy.mockRestore();
  });
});
