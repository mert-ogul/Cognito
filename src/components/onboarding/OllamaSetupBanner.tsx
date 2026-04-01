import type { AppHealth } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type Props = { health: AppHealth | undefined };

export function OllamaSetupBanner({ health }: Props) {
  if (!health || health.ollamaModelReady) {
    return null;
  }
  return (
    <div
      role="region"
      aria-label="Ollama setup"
      className={cn(
        "border-b border-amber-900/50 bg-amber-950/40 px-4 py-2 text-xs text-amber-100",
      )}
    >
      <p className="font-medium">Ollama is not reachable or models are missing.</p>
      <ol className="mt-2 list-inside list-decimal space-y-1 text-amber-200/90">
        <li>
          Install: <code className="rounded bg-black/30 px-1">brew install ollama</code> (macOS) or see{" "}
          <a href="https://ollama.com/download" className="underline" target="_blank" rel="noreferrer">
            ollama.com/download
          </a>
        </li>
        <li>
          Start the daemon: <code className="rounded bg-black/30 px-1">ollama serve</code>
        </li>
        <li>
          Pull models:{" "}
          <code className="rounded bg-black/30 px-1">ollama pull phi4-mini</code> and{" "}
          <code className="rounded bg-black/30 px-1">ollama pull nomic-embed-text</code>
        </li>
      </ol>
    </div>
  );
}
