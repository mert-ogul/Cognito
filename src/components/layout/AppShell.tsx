import { ChatPanel } from "@/components/chat/ChatPanel";
import { PanelErrorBoundary } from "@/components/common/PanelErrorBoundary";
import { Toolbar } from "@/components/common/Toolbar";
import { GraphPanel } from "@/components/graph/GraphPanel";
import { OllamaSetupBanner } from "@/components/onboarding/OllamaSetupBanner";
import { SettingsSheet } from "@/components/settings/SettingsSheet";
import { SourcesPanel } from "@/components/sources/SourcesPanel";
import { RetrievalModeProvider } from "@/context/RetrievalModeContext";
import { useAppHealth } from "@/hooks/useAppHealth";
import { useLibraryRefresh } from "@/hooks/useLibraryRefresh";
import { listDocuments, type ImportProgress } from "@/lib/tauri";
import { listen } from "@/lib/tauri-events";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";

const GRAPH_PANE_KEY = "cognito.graphPaneOpen";

function readGraphPaneOpen(): boolean {
  if (typeof window !== "undefined" && window.innerWidth < 1100) {
    return false;
  }
  try {
    const raw = localStorage.getItem(GRAPH_PANE_KEY);
    if (raw === null) {
      return typeof window === "undefined" || window.innerWidth >= 1100;
    }
    return raw === "1";
  } catch {
    return typeof window === "undefined" || window.innerWidth >= 1100;
  }
}

function writeGraphPaneOpen(open: boolean) {
  try {
    localStorage.setItem(GRAPH_PANE_KEY, open ? "1" : "0");
  } catch {
    /* ignore quota / private mode */
  }
}

export function AppShell() {
  const { data: health } = useAppHealth();
  useLibraryRefresh();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [graphOpen, setGraphOpen] = useState(readGraphPaneOpen);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);

  const docs = useQuery({
    queryKey: ["documents"],
    queryFn: listDocuments,
  });

  const graphQueue = useMemo(
    () =>
      (docs.data ?? []).filter(
        (d) =>
          d.status === "indexed" &&
          (d.graphStatus === "pending" || d.graphStatus === "running"),
      ).length,
    [docs.data],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<ImportProgress>("import-progress", (ev) => {
      setImportProgress(ev.payload);
      if (ev.payload.phase === "done" || ev.payload.status === "completed") {
        setImportProgress(null);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const toggleGraph = () => {
    setGraphOpen((prev) => {
      const next = !prev;
      writeGraphPaneOpen(next);
      return next;
    });
  };

  return (
    <RetrievalModeProvider>
      <div className="flex h-screen flex-col bg-bg text-text">
        <Toolbar
          health={health}
          importProgress={importProgress}
          graphQueue={graphQueue}
          graphOpen={graphOpen}
          onToggleGraph={toggleGraph}
          onOpenSettings={() => setSettingsOpen(true)}
        />
        <div className="flex min-h-0 flex-1">
          <aside className="flex w-[260px] shrink-0 flex-col border-r border-line bg-pane">
            <PanelErrorBoundary>
              <SourcesPanel />
            </PanelErrorBoundary>
          </aside>
          <main className="flex min-h-0 min-w-0 flex-1 flex-col bg-bg">
            <OllamaSetupBanner health={health} />
            <div className="min-h-0 flex-1">
              <PanelErrorBoundary>
                <ChatPanel health={health} />
              </PanelErrorBoundary>
            </div>
          </main>
          {graphOpen ? (
            <aside className="flex w-[360px] shrink-0 flex-col border-l border-line bg-pane">
              <PanelErrorBoundary>
                <GraphPanel />
              </PanelErrorBoundary>
            </aside>
          ) : null}
        </div>
        <SettingsSheet open={settingsOpen} onOpenChange={setSettingsOpen} />
      </div>
    </RetrievalModeProvider>
  );
}
