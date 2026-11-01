import { ChatPanel } from "@/components/chat/ChatPanel";
import { ChatsPanel } from "@/components/chat/ChatsPanel";
import { PanelErrorBoundary } from "@/components/common/PanelErrorBoundary";
import { Toolbar } from "@/components/common/Toolbar";
import { DocumentPage } from "@/components/library/DocumentPage";
import { LibraryPanel } from "@/components/library/LibraryPanel";
import { SidePane } from "@/components/layout/SidePane";
import { OllamaSetupBanner } from "@/components/onboarding/OllamaSetupBanner";
import { SettingsSheet } from "@/components/settings/SettingsSheet";
import { Button } from "@/components/ui/button";
import { useAppHealth } from "@/hooks/useAppHealth";
import { useLibraryRefresh } from "@/hooks/useLibraryRefresh";
import { getSettings, listDocuments, type ImportProgress } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/tauri-events";
import { cn } from "@/lib/utils";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

type SideTab = "library" | "chats";

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

export function AppShell() {
  const { data: health } = useAppHealth();
  useLibraryRefresh();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
  const [sideTab, setSideTab] = useState<SideTab>("library");
  const [chatLoadId, setChatLoadId] = useState<string | null>(null);
  const [chatNewEpoch, setChatNewEpoch] = useState(0);
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
  const settings = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });

  const entitiesQueue = useMemo(() => {
    if (settings.data?.extractEntities === false) return 0;
    return (docs.data ?? []).filter(
      (d) =>
        d.status === "indexed" &&
        (d.entitiesStatus === "pending" || d.entitiesStatus === "running"),
    ).length;
  }, [docs.data, settings.data?.extractEntities]);

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
          <div className="flex shrink-0 gap-1 border-b border-line px-2 py-1">
            <ModeButton
              label="Library"
              active={sideTab === "library"}
              onClick={() => setSideTab("library")}
            />
            <ModeButton
              label="Chats"
              active={sideTab === "chats"}
              onClick={() => setSideTab("chats")}
            />
          </div>
          <div className="min-h-0 flex-1">
            <PanelErrorBoundary>
              {sideTab === "library" ? (
                <LibraryPanel />
              ) : (
                <ChatsPanel
                  onOpenChat={(id) => {
                    setChatLoadId(id);
                    setSideTab("chats");
                  }}
                  onNewChat={() => {
                    setChatLoadId(null);
                    setChatNewEpoch((n) => n + 1);
                    showConversation();
                  }}
                />
              )}
            </PanelErrorBoundary>
          </div>
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
                <ChatPanel
                  health={health}
                  loadConversationId={chatLoadId}
                  newChatEpoch={chatNewEpoch}
                  onStartedNewChat={() => setChatLoadId(null)}
                />
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
