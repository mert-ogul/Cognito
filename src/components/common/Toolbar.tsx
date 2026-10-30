import { Button } from "@/components/ui/button";
import { describeStatus } from "@/lib/status";
import type { AppHealth, ImportProgress } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type Props = {
  health: AppHealth | undefined;
  importProgress: ImportProgress | null;
  entitiesQueue: number;
  sidePaneOpen: boolean;
  onToggleSidePane: () => void;
  onOpenSettings: () => void;
};

export function Toolbar({
  health,
  importProgress,
  entitiesQueue,
  sidePaneOpen,
  onToggleSidePane,
  onOpenSettings,
}: Props) {
  const status = describeStatus(health, importProgress, entitiesQueue);

  return (
    <header className="flex h-10 shrink-0 items-center gap-3 border-b border-line bg-bg px-3">
      <span className="shrink-0 font-medium text-text">Cognito</span>
      <div className="min-w-0 flex-1 truncate text-center">
        {status ? (
          <p
            className={cn(
              "truncate",
              status.tone === "danger" ? "text-danger" : "text-muted",
            )}
          >
            {status.text}
          </p>
        ) : null}
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <Button
          type="button"
          variant="quiet"
          aria-pressed={sidePaneOpen}
          onClick={onToggleSidePane}
        >
          Connections
        </Button>
        <Button type="button" variant="quiet" onClick={onOpenSettings} aria-label="Open settings">
          Settings
        </Button>
      </div>
    </header>
  );
}
