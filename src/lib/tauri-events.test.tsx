import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { e2eEmit, useTauriEvent } from "./tauri-events";

afterEach(() => {
  cleanup();
});

describe("useTauriEvent", () => {
  it("unmounting before listen() resolves leaves no listener behind", async () => {
    const channel = `probe-${crypto.randomUUID()}`;
    let calls = 0;
    function Probe() {
      useTauriEvent(channel, () => {
        calls += 1;
      });
      return null;
    }
    const { unmount } = render(<Probe />);
    unmount();
    await Promise.resolve();
    await Promise.resolve();
    e2eEmit(channel, {});
    expect(calls).toBe(0);
  });
});
