import type { AppHealth } from "@/lib/tauri";

type Props = { health: AppHealth | undefined };

export function OllamaSetupBanner({ health }: Props) {
  if (!health || health.ollamaModelReady) {
    return null;
  }
  const unreachable = !health.ollamaReachable;
  return (
    <div
      role="region"
      aria-label="Ollama setup"
      className="border-b border-line bg-pane px-3 py-2 text-text"
    >
      {unreachable ? (
        <>
          <p className="font-medium">Ollama isn't running</p>
          <ol className="mt-1 list-inside list-decimal space-y-1 text-muted">
            <li>
              Install with <code className="font-mono text-text">brew install ollama</code> on
              macOS, or see <span className="select-all font-mono text-text">ollama.com/download</span>
            </li>
            <li>
              Start it: <code className="font-mono text-text">ollama serve</code>
            </li>
          </ol>
        </>
      ) : (
        <>
          <p className="font-medium">These Ollama models are missing</p>
          <ol className="mt-1 list-inside list-decimal space-y-1 text-muted">
            {health.missingModels.map((name) => (
              <li key={name}>
                Pull: <code className="font-mono text-text">ollama pull {name}</code>
              </li>
            ))}
          </ol>
        </>
      )}
    </div>
  );
}
