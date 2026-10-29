import { ChatPanel } from "@/components/chat/ChatPanel";
import { PanelErrorBoundary } from "@/components/common/PanelErrorBoundary";
import { DebugLogDrawer } from "@/components/debug/DebugLogDrawer";
import { StatusBar } from "@/components/common/StatusBar";
import { GraphPanel } from "@/components/graph/GraphPanel";
import { OllamaSetupBanner } from "@/components/onboarding/OllamaSetupBanner";
import { SettingsSheet } from "@/components/settings/SettingsSheet";
import { SourcesPanel } from "@/components/sources/SourcesPanel";
import { RetrievalModeProvider } from "@/context/RetrievalModeContext";
import { useAppHealth } from "@/hooks/useAppHealth";
import { useLibraryRefresh } from "@/hooks/useLibraryRefresh";
import { useState } from "react";

export function AppShell() {
  const { data: health } = useAppHealth();
  useLibraryRefresh();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [debugOpen, setDebugOpen] = useState(false);

  return (
    <RetrievalModeProvider>
      <div className="flex h-screen flex-col">
        <StatusBar health={health} onOpenSettings={() => setSettingsOpen(true)} />
        <OllamaSetupBanner health={health} />
        <div className="grid min-h-0 flex-1 grid-cols-12 gap-0">
          <section className="col-span-3 min-h-0 border-r border-[var(--border)] p-3">
            <PanelErrorBoundary>
              <SourcesPanel />
            </PanelErrorBoundary>
          </section>
          <section className="col-span-5 flex min-h-0 flex-col border-r border-[var(--border)] p-3">
            <PanelErrorBoundary>
              <ChatPanel health={health} />
            </PanelErrorBoundary>
          </section>
          <section className="col-span-4 min-h-0 p-3">
            <PanelErrorBoundary>
              <GraphPanel />
            </PanelErrorBoundary>
          </section>
        </div>
        <DebugLogDrawer open={debugOpen} onToggle={() => setDebugOpen((o) => !o)} health={health} />
        <SettingsSheet open={settingsOpen} onOpenChange={setSettingsOpen} />
      </div>
    </RetrievalModeProvider>
  );
}
