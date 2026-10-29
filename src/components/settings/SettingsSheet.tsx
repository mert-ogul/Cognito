import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { getSettings, listDocuments, rebuildIndex, updateSettings } from "@/lib/tauri";
import { getVersion } from "@tauri-apps/api/app";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

type ConfirmKind = "save-rebuild" | "rebuild" | null;

export function SettingsSheet({ open, onOpenChange }: Props) {
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["settings"], queryFn: getSettings, enabled: open });
  const docs = useQuery({ queryKey: ["documents"], queryFn: listDocuments, enabled: open });
  const [chat, setChat] = useState("phi4-mini");
  const [embed, setEmbed] = useState("nomic-embed-text");
  const [graphOn, setGraphOn] = useState(true);
  const [ver, setVer] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<ConfirmKind>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
    setGraphOn(data.graphExtractionEnabled);
    setConfirm(null);
    setActionError(null);
  }, [data]);

  const docCount = docs.data?.length ?? 0;
  const embedChanged = !!data && embed.trim() !== data.embedModel;
  const needsRebuildSave = embedChanged && docCount > 0;
  const confirmModel = confirm === "save-rebuild" ? embed.trim() : (data?.embedModel ?? embed);

  const confirmText = useMemo(() => {
    if (!confirm) return null;
    return `All ${docCount} documents will be indexed again with ${confirmModel}, and the graph will be rebuilt.`;
  }, [confirm, confirmModel, docCount]);

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ["settings"] });
    void qc.invalidateQueries({ queryKey: ["health"] });
    void qc.invalidateQueries({ queryKey: ["documents"] });
    void qc.invalidateQueries({ queryKey: ["graph-snapshot"] });
  };

  const savePlain = useMutation({
    mutationFn: () =>
      updateSettings({
        vaultPath: data?.vaultPath ?? null,
        chatModel: chat,
        embedModel: embed,
        graphExtractionEnabled: graphOn,
      }),
    onMutate: () => setActionError(null),
    onSuccess: () => {
      invalidate();
    },
    onError: (e) => setActionError(String(e)),
  });

  const onSaveClick = () => {
    setActionError(null);
    if (needsRebuildSave) {
      setConfirm("save-rebuild");
      return;
    }
    savePlain.mutate();
  };

  const cancelConfirm = () => {
    if (confirm === "save-rebuild" && data) {
      setEmbed(data.embedModel);
    }
    setConfirm(null);
  };

  const runConfirmed = async () => {
    if (!data || !confirm) return;
    setBusy(true);
    setActionError(null);
    try {
      if (confirm === "save-rebuild") {
        await updateSettings({
          vaultPath: data.vaultPath ?? null,
          chatModel: chat,
          embedModel: data.embedModel,
          graphExtractionEnabled: graphOn,
        });
        await rebuildIndex(embed.trim());
      } else {
        await rebuildIndex();
      }
      setConfirm(null);
      invalidate();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
        </DialogHeader>
        <p className="text-xs text-[var(--muted)]">
          Cognito works offline for your library. No account is required.
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
        {confirm ? (
          <div className="space-y-2 rounded-md border border-[var(--border)] p-3">
            <p className="text-xs">{confirmText}</p>
            <div className="flex flex-wrap gap-2">
              <Button type="button" size="sm" disabled={busy} onClick={() => void runConfirmed()}>
                {busy ? "Working…" : "Confirm"}
              </Button>
              <Button type="button" size="sm" variant="outline" disabled={busy} onClick={cancelConfirm}>
                Cancel
              </Button>
            </div>
          </div>
        ) : null}
        {actionError ? (
          <p className="text-xs text-red-500" role="alert">
            {actionError}
          </p>
        ) : null}
        <div className="flex flex-wrap justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={busy || savePlain.isPending || docCount === 0}
            onClick={() => {
              setActionError(null);
              setConfirm("rebuild");
            }}
          >
            Rebuild index
          </Button>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          <Button
            type="button"
            onClick={onSaveClick}
            disabled={busy || savePlain.isPending || !!confirm}
          >
            {needsRebuildSave ? "Save and rebuild index" : "Save"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
