import { Button } from "@/components/ui/button";
import type { AppHealth } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type Props = {
  open: boolean;
  onToggle: () => void;
  health: AppHealth | undefined;
};

/** Collapsible local debug strip (health JSON only — no remote logging). */
export function DebugLogDrawer({ open, onToggle, health }: Props) {
  return (
    <div className="border-t border-[var(--border)] bg-[var(--surface)]">
      <div className="flex items-center justify-between px-3 py-1">
        <span className="text-[10px] uppercase text-[var(--muted)]">Debug</span>
        <Button type="button" variant="ghost" size="sm" className="h-7 text-[10px]" onClick={onToggle}>
          {open ? "Hide" : "Show"} health snapshot
        </Button>
      </div>
      {open ? (
        <pre
          className={cn(
            "max-h-36 overflow-auto border-t border-[var(--border)] px-3 py-2 font-mono text-[10px] text-[var(--muted)]",
          )}
        >
          {JSON.stringify(health ?? {}, null, 2)}
        </pre>
      ) : null}
    </div>
  );
}
