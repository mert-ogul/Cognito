import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { FIRST_IMPORT_DOC_CAP } from "@/components/sources/constants";
import { DocumentStatusBadge } from "@/components/sources/DocumentStatusBadge";
import { DocumentTypeIcon } from "@/components/sources/DocumentTypeIcon";
import {
  importDocuments,
  listDocuments,
  retryFailedImport,
  selectVaultFolder,
  type DocumentRow,
  type GraphExtractionProgress,
  type ImportProgress,
} from "@/lib/tauri";
import { listen } from "@/lib/tauri-events";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";

function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

function formatWhen(iso: string | null | undefined): string {
  if (!iso) return "—";
  return iso.slice(0, 19).replace("T", " ");
}

export function SourcesPanel() {
  const qc = useQueryClient();
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [graphProgress, setGraphProgress] = useState<GraphExtractionProgress | null>(null);
  const [dragOver, setDragOver] = useState(false);

  const docs = useQuery({
    queryKey: ["documents"],
    queryFn: listDocuments,
  });

  const pickVault = useMutation({
    mutationFn: selectVaultFolder,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["health"] });
      void qc.invalidateQueries({ queryKey: ["documents"] });
    },
  });

  const runImport = useMutation({
    mutationFn: (folder: string | null) => importDocuments(folder),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["documents"] });
    },
  });

  const retry = useMutation({
    mutationFn: retryFailedImport,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["documents"] });
    },
  });

  const invalidateDocs = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ["documents"] });
  }, [qc]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let unGraph: (() => void) | undefined;
    void listen<ImportProgress>("import-progress", (ev) => {
      setProgress(ev.payload);
      if (ev.payload.phase === "done" || ev.payload.status === "completed") {
        setGraphProgress(null);
      }
      invalidateDocs();
    }).then((fn) => {
      unlisten = fn;
    });
    void listen<GraphExtractionProgress>("graph-extraction-progress", (ev) => {
      setGraphProgress(ev.payload);
      invalidateDocs();
    }).then((fn) => {
      unGraph = fn;
    });
    return () => {
      unlisten?.();
      unGraph?.();
    };
  }, [invalidateDocs]);

  useEffect(() => {
    let un: (() => void) | undefined;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        un = await getCurrentWindow().onDragDropEvent((event) => {
          if (event.payload.type === "enter" || event.payload.type === "over") {
            setDragOver(true);
          } else if (event.payload.type === "leave") {
            setDragOver(false);
          } else if (event.payload.type === "drop") {
            setDragOver(false);
            const first = event.payload.paths[0];
            if (first) {
              runImport.mutate(first);
            }
          }
        });
      } catch {
        /* Web preview / non-Tauri: folder drop is unavailable */
      }
    })();
    return () => {
      un?.();
    };
  }, [runImport]);

  const rows: DocumentRow[] = docs.data ?? [];
  const showEmpty = rows.length === 0 && !docs.isLoading;
  const importPct = useMemo(() => {
    if (!progress?.totalFiles) return 0;
    return Math.round((100 * progress.completedFiles) / progress.totalFiles);
  }, [progress]);

  const underCap = rows.length < FIRST_IMPORT_DOC_CAP;
  const showImportMore = rows.length > 0 && underCap;

  return (
    <Card
      className={`flex h-full min-h-0 flex-col ${dragOver ? "ring-2 ring-[var(--ring)]" : ""}`}
      onDragEnter={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
    >
      <CardHeader>
        <CardTitle>Sources</CardTitle>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3">
        {progress && progress.totalFiles > 0 ? (
          <div className="space-y-1">
            <Progress value={importPct} aria-label="Import progress" />
            <p className="text-xs text-[var(--muted)]">
              Import {progress.completedFiles}/{progress.totalFiles} · {progress.phase}
              {progress.path ? ` · ${fileName(progress.path)}` : ""}
              {progress.errorMessage ? ` · ${progress.errorMessage}` : ""}
            </p>
          </div>
        ) : progress ? (
          <p className="text-xs text-[var(--muted)]">
            Import {progress.completedFiles}/{progress.totalFiles || "…"} · {progress.phase}
            {progress.path ? ` · ${fileName(progress.path)}` : ""}
          </p>
        ) : null}
        {graphProgress && (
          <p className="text-xs text-[var(--muted)]">
            Graph {graphProgress.phase}
            {graphProgress.detail ? ` · ${graphProgress.detail}` : ""}
          </p>
        )}
        {showEmpty && (
          <div className="rounded-md border border-dashed border-[var(--border)] p-3 text-xs text-[var(--muted)]">
            <p className="mb-2 font-medium text-[var(--foreground)]">Local library</p>
            <ul className="list-inside list-disc space-y-1">
              <li>Everything stays on this device — no accounts, no cloud sync.</li>
              <li>PDF and Markdown only.</li>
              <li>First library capped at {FIRST_IMPORT_DOC_CAP} documents.</li>
              <li>Drop a folder here or choose a vault, then import.</li>
            </ul>
          </div>
        )}
        <p className="text-xs text-[var(--muted)]">
          Files are indexed in place, metadata lives under the app data directory.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={pickVault.isPending}
            onClick={() => pickVault.mutate()}
          >
            {pickVault.isPending ? "Opening…" : "Choose vault folder"}
          </Button>
          <Button
            type="button"
            variant="default"
            disabled={runImport.isPending}
            onClick={() => runImport.mutate(null)}
          >
            {runImport.isPending ? "Importing…" : "Import vault"}
          </Button>
          {showImportMore ? (
            <Button
              type="button"
              variant="outline"
              disabled={runImport.isPending}
              onClick={() => runImport.mutate(null)}
            >
              Import more
            </Button>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={retry.isPending}
            onClick={() => retry.mutate()}
          >
            Retry failed
          </Button>
        </div>
        {!underCap ? (
          <p className="text-[10px] text-amber-600 dark:text-amber-400">
            Document cap reached ({FIRST_IMPORT_DOC_CAP}). Remove files to add different ones.
          </p>
        ) : null}
        <div className="min-h-0 flex-1 overflow-auto rounded-md border border-[var(--border)]">
          <table className="w-full text-left text-xs">
            <thead className="sticky top-0 bg-[var(--background)]">
              <tr className="border-b border-[var(--border)]">
                <th className="p-2 font-medium">File</th>
                <th className="p-2 font-medium w-10" aria-label="Type" />
                <th className="p-2 font-medium">Status</th>
                <th className="p-2 font-medium">Pages</th>
                <th className="p-2 font-medium">Modified</th>
                <th className="p-2 font-medium">Indexed</th>
                <th className="p-2 font-medium">Graph</th>
                <th className="p-2 w-8" aria-label="Retry" />
              </tr>
            </thead>
            <tbody>
              {rows.map((d) => (
                <tr key={d.id} className="border-b border-[var(--border)]/60">
                  <td className="p-2 align-top">
                    <span className="line-clamp-2 break-all" title={d.path}>
                      {fileName(d.path)}
                    </span>
                    {d.errorMessage ? (
                      <span className="mt-1 inline-block rounded bg-red-500/15 px-1.5 py-0.5 text-[10px] text-red-600 dark:text-red-400">
                        Error
                      </span>
                    ) : null}
                  </td>
                  <td className="p-2 align-top">
                    <DocumentTypeIcon fileType={d.fileType} />
                  </td>
                  <td className="p-2 align-top">
                    <DocumentStatusBadge status={d.status} errorMessage={d.errorMessage} />
                  </td>
                  <td className="p-2 align-top">
                    {d.fileType.toLowerCase() === "pdf" ? (d.pageCount ?? "—") : "—"}
                  </td>
                  <td className="p-2 align-top whitespace-nowrap">{formatWhen(d.modifiedAt)}</td>
                  <td className="p-2 align-top whitespace-nowrap">{formatWhen(d.indexedAt)}</td>
                  <td className="p-2 align-top">{d.graphStatus}</td>
                  <td className="p-2 align-top">
                    {(d.status === "failed" || d.errorMessage) && (
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 px-1 text-[10px]"
                        disabled={retry.isPending}
                        onClick={() => retry.mutate()}
                        aria-label={`Retry failed imports including ${fileName(d.path)}`}
                      >
                        Retry
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  );
}
