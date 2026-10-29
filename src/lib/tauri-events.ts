import { listen as tauriListen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

const e2e =
  import.meta.env.VITE_E2E === true ||
  import.meta.env.VITE_E2E === "true" ||
  import.meta.env.MODE === "test";
const e2eListeners = new Map<string, Set<(payload: unknown) => void>>();

/** Tauri event bridge; in E2E/test builds uses in-process listeners for Playwright/Vitest. */
export async function listen<T>(
  channel: string,
  handler: (event: { payload: T }) => void,
): Promise<() => void> {
  if (e2e) {
    if (!e2eListeners.has(channel)) {
      e2eListeners.set(channel, new Set());
    }
    const set = e2eListeners.get(channel)!;
    const fn = (payload: unknown) => handler({ payload: payload as T });
    set.add(fn);
    return () => {
      set.delete(fn);
    };
  }
  return tauriListen(channel, handler);
}

/** Subscribe to a Tauri channel; keeps the latest handler and always unsubscribes on cleanup. */
export function useTauriEvent<T>(
  channel: string,
  handler: (event: { payload: T }) => void,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<T>(channel, (event) => {
      handlerRef.current(event);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [channel]);
}

/** Emit a Tauri-style event (E2E/test builds only). */
export function e2eEmit<T>(channel: string, payload: T): void {
  e2eListeners.get(channel)?.forEach((fn) => {
    fn(payload);
  });
}
