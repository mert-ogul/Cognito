import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { getSettings, updateSettings } from "@/lib/tauri";
import { getVersion } from "@tauri-apps/api/app";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function SettingsSheet({ open, onOpenChange }: Props) {
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["settings"], queryFn: getSettings, enabled: open });
  const [chat, setChat] = useState("phi4-mini");
  const [embed, setEmbed] = useState("nomic-embed-text");
  const [tel, setTel] = useState(false);
  const [crash, setCrash] = useState(false);
  const [graphOn, setGraphOn] = useState(true);
  const [ver, setVer] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    void (async () => {
      try {
        setVer(await getVersion());
      } catch {
        setVer("preview");
      }
    })();
  }, [open]);

  useEffect(() => {
    if (!data) return;
    setChat(data.chatModel);
    setEmbed(data.embedModel);
    setTel(data.telemetryOptIn);
    setCrash(data.crashReportingOptIn);
    setGraphOn(data.graphExtractionEnabled);
  }, [data]);

  const save = useMutation({
    mutationFn: () =>
      updateSettings({
        vaultPath: data?.vaultPath ?? null,
        chatModel: chat,
        embedModel: embed,
        telemetryOptIn: tel,
        crashReportingOptIn: crash,
        graphExtractionEnabled: graphOn,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["health"] });
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
        </DialogHeader>
        <p className="text-xs text-[var(--muted)]">
          Cognito works offline for your library. No account is required. Telemetry and crash reports are opt-in and off
          by default.
        </p>
        <div className="space-y-2">
          <Label htmlFor="vault-path">Vault folder</Label>
          <p id="vault-path" className="rounded-md border border-[var(--border)] bg-[var(--surface)] px-2 py-1.5 font-mono text-[11px] break-all">
            {data?.vaultPath ?? "Not configured"}
          </p>
        </div>
        <div className="grid gap-3">
          <div className="space-y-1">
            <Label htmlFor="chat-model">Chat model</Label>
            <Input id="chat-model" value={chat} onChange={(e) => setChat(e.target.value)} autoComplete="off" />
          </div>
          <div className="space-y-1">
            <Label htmlFor="embed-model">Embedding model</Label>
            <Input id="embed-model" value={embed} onChange={(e) => setEmbed(e.target.value)} autoComplete="off" />
          </div>
        </div>
        <div className="flex items-center justify-between gap-3 rounded-md border border-[var(--border)] px-3 py-2">
          <div>
            <p className="text-sm font-medium">Telemetry</p>
            <p className="text-[10px] text-[var(--muted)]">Product analytics (off by default)</p>
          </div>
          <Switch checked={tel} onCheckedChange={setTel} aria-label="Telemetry opt-in" />
        </div>
        <div className="flex items-center justify-between gap-3 rounded-md border border-[var(--border)] px-3 py-2">
          <div>
            <p className="text-sm font-medium">Crash reporting</p>
            <p className="text-[10px] text-[var(--muted)]">Send crash dumps when enabled (off by default)</p>
          </div>
          <Switch checked={crash} onCheckedChange={setCrash} aria-label="Crash reporting opt-in" />
        </div>
        <div className="flex items-center justify-between gap-3 rounded-md border border-[var(--border)] px-3 py-2">
          <div>
            <p className="text-sm font-medium">Build knowledge graph</p>
            <p className="text-[10px] text-[var(--muted)]">
              Runs the chat model on every chunk. Slow on large libraries.
            </p>
          </div>
          <Switch
            checked={graphOn}
            onCheckedChange={setGraphOn}
            aria-label="Build knowledge graph"
          />
        </div>
        <div className="rounded-md bg-[var(--surface)] p-3 text-[10px] text-[var(--muted)]">
          <p className="font-mono">App version: {ver ?? "…"}</p>
          <p className="mt-1">First-import document cap: 100 (MVP).</p>
        </div>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          <Button type="button" onClick={() => save.mutate()} disabled={save.isPending}>
            Save
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
