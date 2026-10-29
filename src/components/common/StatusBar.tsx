import { Button } from "@/components/ui/button";
import { useRetrievalMode } from "@/context/RetrievalModeContext";
import { cn } from "@/lib/utils";
import type { AppHealth } from "@/lib/tauri";

type Props = {
  health: AppHealth | undefined;
  onOpenSettings: () => void;
};

export function StatusBar({ health, onOpenSettings }: Props) {
  const { lastRetrievalMode } = useRetrievalMode();
  const modelReady = health?.chatModelWarmed && !health?.warmUpPending;
  return (
    <div
      className={cn(
        "flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)]",
        "bg-[var(--surface)] px-4 py-2",
      )}
    >
      <div className="flex flex-col gap-0.5">
        <span className="text-sm font-semibold tracking-tight">Cognito</span>
        <span className="text-xs text-[var(--muted)]">Local-first · hybrid chat · read-only graph</span>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Pill label="Sidecar" ok={health?.sidecarReachable} />
        <Pill label="Ollama" ok={health?.ollamaModelReady} />
        <Pill label="Model ready" ok={modelReady} detail={health?.warmUpError} />
        <Pill label="SQLite" ok={health?.sqliteOk} />
        <Pill label="Vault" ok={health?.vaultConfigured} />
        <RetrievalPill mode={lastRetrievalMode} />
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-8 text-xs"
          onClick={onOpenSettings}
          aria-label="Open settings"
        >
          Settings
        </Button>
      </div>
    </div>
  );
}

function RetrievalPill({ mode }: { mode: string | null }) {
  if (!mode) {
    return (
      <span className="rounded-full bg-[var(--background)] px-2 py-0.5 font-mono text-[10px] uppercase text-[var(--muted)]">
        Retrieval: —
      </span>
    );
  }
  const hybrid = mode === "hybrid";
  return (
    <span
      className={cn(
        "rounded-full px-2 py-0.5 font-mono text-[10px] uppercase",
        hybrid ? "bg-sky-950/80 text-sky-200" : "bg-stone-900/80 text-stone-300",
      )}
    >
      Retrieval: {hybrid ? "hybrid" : mode === "vector_only" ? "vector" : mode}
    </span>
  );
}

function Pill({
  label,
  ok,
  detail,
}: {
  label: string;
  ok?: boolean;
  detail?: string | null;
}) {
  const title = detail && ok === false ? detail : undefined;
  return (
    <span
      title={title}
      className={cn(
        "rounded-full px-2 py-0.5 font-mono text-[10px] uppercase",
        ok ? "bg-emerald-950/80 text-emerald-300" : "bg-amber-950/80 text-amber-200",
      )}
    >
      {label}: {ok ? "ok" : "wait"}
    </span>
  );
}
