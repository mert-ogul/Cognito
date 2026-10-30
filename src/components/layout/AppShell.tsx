import { ChatPanel } from "@/components/chat/ChatPanel";
import { PanelErrorBoundary } from "@/components/common/PanelErrorBoundary";
import { Toolbar } from "@/components/common/Toolbar";
import { DocumentPage } from "@/components/library/DocumentPage";
import { LibraryPanel } from "@/components/library/LibraryPanel";
import { SidePane } from "@/components/layout/SidePane";
import { OllamaSetupBanner } from "@/components/onboarding/OllamaSetupBanner";
import { SettingsSheet } from "@/components/settings/SettingsSheet";
import { useAppHealth } from "@/hooks/useAppHealth";
import { useLibraryRefresh } from "@/hooks/useLibraryRefresh";
import { listDocuments, type ImportProgress } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/tauri-events";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

export function AppShell() {
  const { data: health } = useAppHealth();
  useLibraryRefresh();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
  const paneOpen = useSidePaneStore((s) => s.open);
  const toggleOpen = useSidePaneStore((s) => s.toggleOpen);
  const view = useMainViewStore((s) => s.view);
  const documentId = useMainViewStore((s) => s.documentId);
  const showConversation = useMainViewStore((s) => s.showConversation);
  const setAskAbout = useChatIntentStore((s) => s.setAskAbout);

  const docs = useQuery({
    queryKey: ["documents"],
    queryFn: listDocuments,
  });

  const entitiesQueue = useMemo(
    () =>
      (docs.data ?? []).filter(
        (d) =>
          d.status === "indexed" &&
          (d.entitiesStatus === "pending" || d.entitiesStatus === "running"),
      ).length,
    [docs.data],
  );

  useTauriEvent<ImportProgress>("import-progress", (ev) => {
    setImportProgress(ev.payload);
    if (ev.payload.phase === "done" || ev.payload.status === "completed") {
      setImportProgress(null);
    }
  });

  return (
    <div className="flex h-screen flex-col bg-bg text-text">
      <Toolbar
        health={health}
        importProgress={importProgress}
        entitiesQueue={entitiesQueue}
        sidePaneOpen={paneOpen}
        onToggleSidePane={toggleOpen}
        onOpenSettings={() => setSettingsOpen(true)}
      />
      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[260px] shrink-0 flex-col border-r border-line bg-pane">
          <PanelErrorBoundary>
            <LibraryPanel />
          </PanelErrorBoundary>
        </aside>
        <main className="flex min-h-0 min-w-0 flex-1 flex-col bg-bg">
          <OllamaSetupBanner health={health} />
          <div className="relative min-h-0 flex-1">
            <PanelErrorBoundary>
              <div
                className={
                  view === "document" && documentId ? "hidden h-full" : "h-full"
                }
              >
                <ChatPanel health={health} />
              </div>
              {view === "document" && documentId ? (
                <div className="absolute inset-0 h-full">
                  <DocumentPage
                    documentId={documentId}
                    onBackToConversation={showConversation}
                    onAskAboutDocument={(scopeDoc) => setAskAbout([scopeDoc])}
                  />
                </div>
              ) : null}
            </PanelErrorBoundary>
          </div>
        </main>
        {paneOpen ? (
          <aside className="flex w-[360px] shrink-0 flex-col border-l border-line bg-pane">
            <PanelErrorBoundary>
              <SidePane />
            </PanelErrorBoundary>
          </aside>
        ) : null}
      </div>
      <SettingsSheet open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  );
}
