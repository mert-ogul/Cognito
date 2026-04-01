type Props = { modelReady: boolean };

export function ChatWelcome({ modelReady }: Props) {
  return (
    <div className="rounded-lg border border-[var(--border)] bg-[var(--surface)]/50 p-4 text-sm">
      <p className="font-medium text-[var(--foreground)]">Welcome to Cognito</p>
      <ul className="mt-3 list-inside list-disc space-y-2 text-[var(--muted)]">
        <li>Imports and embeddings stay on this machine — works offline once models are pulled.</li>
        <li>PDF and Markdown only.</li>
        <li>
          Chat stays disabled until the chat model has finished warming up{modelReady ? " (ready now)." : " (watch the status bar)."}
        </li>
      </ul>
    </div>
  );
}
