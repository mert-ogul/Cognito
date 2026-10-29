import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import {
  clearLibrary,
  getSettings,
  listDocuments,
  openLogsFolder,
  rebuildIndex,
  selectVaultFolder,
  setVault,
  updateSettings,
} from "@/lib/tauri";
import { getVersion } from "@tauri-apps/api/app";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

type ConfirmKind = "save-rebuild" | "rebuild" | "change-folder" | "remove-library" | null;

export function SettingsSheet({ open, onOpenChange }: Props) {
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["settings"], queryFn: getSettings, enabled: open });
  const docs = useQuery({ queryKey: ["documents"], queryFn: listDocuments, enabled: open });
  const [chat, setChat] = useState("phi4-mini");
  const [embed, setEmbed] = useState("nomic-embed-text");
  const [graphOn, setGraphOn] = useState(true);
  const [ver, setVer] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<ConfirmKind>(null);
  const [pendingFolder, setPendingFolder] = useState<string | null>(null);
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
    setPendingFolder(null);
    setActionError(null);
  }, [data]);

  const docCount = docs.data?.length ?? 0;
  const folderLabel = data?.vaultPath ?? "Not configured";
  const embedChanged = !!data && embed.trim() !== data.embedModel;
  const needsRebuildSave = embedChanged && docCount > 0;
  const confirmModel = confirm === "save-rebuild" ? embed.trim() : (data?.embedModel ?? embed);

  const confirmText = useMemo(() => {
    if (!confirm) return null;
    if (confirm === "change-folder" && pendingFolder) {
      return `Switching to ${pendingFolder} removes the ${docCount} documents from ${folderLabel} from Cognito. Your files stay where they are.`;
    }
    if (confirm === "rebuild") {
      return `Every document in ${folderLabel} is indexed again and the graph is rebuilt.`;
    }
    if (confirm === "remove-library") {
      return `Cognito forgets ${folderLabel} and deletes its index and graph. Your files stay where they are.`;
    }
    return `All ${docCount} documents will be indexed again with ${confirmModel}, and the graph will be rebuilt.`;
  }, [confirm, confirmModel, docCount, folderLabel, pendingFolder]);

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
    setPendingFolder(null);
    setConfirm(null);
  };

  const onChangeFolder = async () => {
    setActionError(null);
    try {
      const picked = await selectVaultFolder();
      if (!picked) return;
      if (docCount === 0) {
        setBusy(true);
        try {
          await setVault(picked);
          invalidate();
        } finally {
          setBusy(false);
        }
        return;
      }
      setPendingFolder(picked);
      setConfirm("change-folder");
    } catch (e) {
      setActionError(String(e));
    }
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
      } else if (confirm === "rebuild") {
        await rebuildIndex();
      } else if (confirm === "change-folder" && pendingFolder) {
        await setVault(pendingFolder);
      } else if (confirm === "remove-library") {
        await clearLibrary();
      }
      setPendingFolder(null);
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

        <section className="space-y-2">
          <h3 className="font-medium text-text">Library</h3>
          <p className="break-all border border-line bg-pane px-2 py-1.5 font-mono text-[12px]">
            {folderLabel}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="secondary"
              disabled={busy || !!confirm}
              onClick={() => void onChangeFolder()}
            >
              Change folder
            </Button>
            <Button
              type="button"
              variant="secondary"
              disabled={busy || !!confirm || docCount === 0 || !data?.vaultPath}
              onClick={() => {
                setActionError(null);
                setConfirm("rebuild");
              }}
            >
              Rebuild index
            </Button>
            <Button
              type="button"
              variant="secondary"
              disabled={busy || !!confirm || !data?.vaultPath}
              onClick={() => {
                setActionError(null);
                setConfirm("remove-library");
              }}
            >
              Remove library
            </Button>
          </div>
        </section>

        <section className="space-y-2">
          <h3 className="font-medium text-text">Models</h3>
          <div className="space-y-1">
            <Label htmlFor="chat-model">Chat model</Label>
            <Input id="chat-model" value={chat} onChange={(e) => setChat(e.target.value)} autoComplete="off" />
          </div>
          <div className="space-y-1">
            <Label htmlFor="embed-model">Embedding model</Label>
            <Input id="embed-model" value={embed} onChange={(e) => setEmbed(e.target.value)} autoComplete="off" />
          </div>
        </section>

        <section className="space-y-2">
          <h3 className="font-medium text-text">Knowledge graph</h3>
          <div className="flex items-center justify-between gap-3 border border-line px-3 py-2">
            <div>
              <p className="text-text">Build knowledge graph</p>
              <p className="text-muted">
                Runs the chat model on every chunk. Slow on large libraries.
              </p>
            </div>
            <Switch
              checked={graphOn}
              onCheckedChange={setGraphOn}
              aria-label="Build knowledge graph"
            />
          </div>
        </section>

        <section className="space-y-2">
          <h3 className="font-medium text-text">About</h3>
          <p className="text-muted">Version {ver ?? "…"}</p>
          <Button
            type="button"
            variant="secondary"
            onClick={() => {
              void openLogsFolder().catch((e) => setActionError(String(e)));
            }}
          >
            Open logs folder
          </Button>
        </section>

        {confirm ? (
          <div className="space-y-2 border border-line p-3">
            <p className="text-text">{confirmText}</p>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="primary" disabled={busy} onClick={() => void runConfirmed()}>
                {busy ? "Working…" : "Confirm"}
              </Button>
              <Button type="button" variant="secondary" disabled={busy} onClick={cancelConfirm}>
                Cancel
              </Button>
            </div>
          </div>
        ) : null}
        {actionError ? (
          <p className="text-danger" role="alert">
            {actionError}
          </p>
        ) : null}
        <div className="flex flex-wrap justify-end gap-2">
          <Button type="button" variant="secondary" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          <Button
            type="button"
            variant="primary"
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
