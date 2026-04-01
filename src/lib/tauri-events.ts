import { listen as tauriListen } from "@tauri-apps/api/event";

const e2e = import.meta.env.VITE_E2E === true || import.meta.env.VITE_E2E === "true";
const e2eListeners = new Map<string, Set<(payload: unknown) => void>>();

/** Tauri event bridge; in E2E builds uses in-process listeners for Playwright. */
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

/** Emit a Tauri-style event (E2E builds only). */
export function e2eEmit<T>(channel: string, payload: T): void {
  e2eListeners.get(channel)?.forEach((fn) => {
    fn(payload);
  });
}
