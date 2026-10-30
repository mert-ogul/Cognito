import { Button } from "@/components/ui/button";
import { ConnectionsPanel } from "@/components/connections/ConnectionsPanel";
import { PassageView } from "@/components/layout/PassageView";
import { cn } from "@/lib/utils";
import { useSidePaneStore } from "@/stores/sidePaneStore";

export function SidePane() {
  const mode = useSidePaneStore((s) => s.mode);
  const passage = useSidePaneStore((s) => s.passage);
  const setMode = useSidePaneStore((s) => s.setMode);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 gap-1 border-b border-line px-2 py-1">
        <ModeButton
          label="Passage"
          active={mode === "passage"}
          onClick={() => setMode("passage")}
        />
        <ModeButton
          label="Connections"
          active={mode === "connections"}
          onClick={() => setMode("connections")}
        />
      </div>
      <div className="min-h-0 flex-1">
        {mode === "passage" ? (
          passage ? (
            <PassageView passage={passage} />
          ) : (
            <p className="p-3 text-muted">Open a source from the conversation to read a passage.</p>
          )
        ) : (
          <ConnectionsPanel />
        )}
      </div>
    </div>
  );
}

function ModeButton({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      variant="quiet"
      aria-pressed={active}
      className={cn(
        "rounded-none border-b-2 border-transparent px-2",
        active ? "border-accent text-text" : "text-muted",
      )}
      onClick={onClick}
    >
      {label}
    </Button>
  );
}
