import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { FIRST_IMPORT_DOC_CAP } from "@/components/sources/constants";
import {
  getSettings,
  importDocuments,
  listDocuments,
  retryFailedImport,
  selectVaultFolder,
  setVault,
  type DocumentRow,
  type GraphExtractionProgress,
  type ImportProgress,
} from "@/lib/tauri";
import { listen } from "@/lib/tauri-events";
import { cn } from "@/lib/utils";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";

function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

function folderName(path: string | null | undefined): string {
  if (!path) return "";
  const trimmed = path.replace(/[/\\]+$/, "");
  const i = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return i >= 0 ? trimmed.slice(i + 1) : trimmed;
}

function typeLine(d: DocumentRow): string {
  const ft = d.fileType.toLowerCase();
  if (ft === "pdf") {
    return d.pageCount != null ? `PDF, ${d.pageCount} pages` : "PDF";
  }
  return "Markdown";
}

function sortByName(a: DocumentRow, b: DocumentRow): number {
  return fileName(a.path).localeCompare(fileName(b.path), undefined, { sensitivity: "base" });
}

export function SourcesPanel() {
  const qc = useQueryClient();
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [graphProgress, setGraphProgress] = useState<GraphExtractionProgress | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const docs = useQuery({
    queryKey: ["documents"],
    queryFn: listDocuments,
  });

  const settings = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });

  const chooseFolder = useMutation({
    mutationFn: async () => {
      const path = await selectVaultFolder();
      if (!path) return null;
      return setVault(path);
    },
    onMutate: () => {
      setActionError(null);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["health"] });
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["documents"] });
    },
    onError: (error) => {
      setActionError(String(error));
    },
  });

  const runImport = useMutation({
    mutationFn: (folder: string | null) => importDocuments(folder),
    onMutate: () => {
      setActionError(null);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["documents"] });
    },
    onError: (error) => {
      setActionError(String(error));
    },
  });

  const retry = useMutation({
    mutationFn: retryFailedImport,
    onMutate: () => {
      setActionError(null);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["documents"] });
    },
    onError: (error) => {
      setActionError(String(error));
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

  const rows = useMemo(
    () => [...(docs.data ?? [])].sort(sortByName),
    [docs.data],
  );
  const vaultPath = settings.data?.vaultPath ?? null;
  const vaultSet = !!vaultPath;
  const underCap = rows.length < FIRST_IMPORT_DOC_CAP;
  const failedCount = rows.filter((d) => d.status === "failed" || d.errorMessage).length;
  const graphWaiting = useMemo(
    () =>
      rows.filter(
        (d) =>
          d.status === "indexed" &&
          (d.graphStatus === "pending" || d.graphStatus === "running"),
      ).length,
    [rows],
  );
  const graphEnabled = settings.data?.graphExtractionEnabled !== false;
  const importActive =
    !!progress &&
    progress.totalFiles > 0 &&
    progress.status !== "completed" &&
    progress.phase !== "done";
  const importPct = importActive
    ? Math.round((100 * progress.completedFiles) / progress.totalFiles)
    : 0;

  return (
    <div
      className={cn(
        "flex h-full min-h-0 flex-col",
        dragOver ? "outline outline-2 outline-accent outline-offset-[-2px]" : "",
      )}
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
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
        <div className="min-w-0 flex-1">
          <p className="font-medium text-text">Library</p>
          {vaultSet ? (
            <p className="truncate text-muted" title={vaultPath}>
              {folderName(vaultPath)}
            </p>
          ) : null}
        </div>
        {vaultSet ? (
          <Button
            type="button"
            variant="quiet"
            disabled={runImport.isPending}
            onClick={() => runImport.mutate(null)}
          >
            {runImport.isPending ? "Importing…" : "Import"}
          </Button>
        ) : null}
      </div>

      {!vaultSet ? (
        <div className="flex flex-1 flex-col gap-3 p-3">
          <p className="text-muted">
            Pick a folder of PDF and Markdown files. Cognito indexes it on this computer.
          </p>
          <Button
            type="button"
            variant="primary"
            disabled={chooseFolder.isPending}
            onClick={() => chooseFolder.mutate()}
          >
            {chooseFolder.isPending ? "Opening…" : "Choose a folder"}
          </Button>
          {actionError ? (
            <p className="text-danger" role="alert">
              {actionError}
            </p>
          ) : null}
        </div>
      ) : (
        <>
          <ul className="min-h-0 flex-1 overflow-auto">
            {rows.map((d) => {
              const indexing =
                d.status === "pending" || d.status === "parsing" || d.status === "embedding";
              const failed = d.status === "failed" || !!d.errorMessage;
              return (
                <li key={d.id} className="border-b border-line px-3 py-2">
                  <p className="truncate text-text" title={d.path}>
                    {fileName(d.path)}
                  </p>
                  <p className="text-muted">{typeLine(d)}</p>
                  {indexing ? <p className="text-muted">Indexing</p> : null}
                  {failed ? (
                    <div className="mt-0.5 flex flex-wrap items-center gap-2">
                      <p className="text-danger" title={d.errorMessage ?? undefined}>
                        Couldn&apos;t read this file
                      </p>
                      <Button
                        type="button"
                        variant="quiet"
                        disabled={retry.isPending}
                        onClick={() => retry.mutate()}
                        aria-label={`Retry failed imports including ${fileName(d.path)}`}
                      >
                        Retry
                      </Button>
                    </div>
                  ) : null}
                  {d.graphStatus === "failed" ? (
                    <p className="text-danger" title={d.graphError ?? undefined}>
                      Graph failed
                    </p>
                  ) : null}
                </li>
              );
            })}
          </ul>

          <div className="shrink-0 space-y-1.5 border-t border-line px-3 py-2">
            {importActive ? (
              <div className="space-y-1">
                <p className="text-muted">
                  Indexing {progress.completedFiles} of {progress.totalFiles}
                  {progress.path ? `, ${fileName(progress.path)}` : ""}
                </p>
                <Progress value={importPct} aria-label="Import progress" />
              </div>
            ) : null}
            {graphEnabled ? (
              graphWaiting > 0 || graphProgress ? (
                <p className="text-muted">
                  Building graph, {graphWaiting} file{graphWaiting === 1 ? "" : "s"} left
                </p>
              ) : null
            ) : (
              <p className="text-muted">Graph building is off</p>
            )}
            {failedCount > 0 ? (
              <Button
                type="button"
                variant="quiet"
                disabled={retry.isPending}
                onClick={() => retry.mutate()}
              >
                Retry failed
              </Button>
            ) : null}
            {!underCap ? (
              <p className="text-muted">
                Document cap reached ({FIRST_IMPORT_DOC_CAP}). Remove files to add different ones.
              </p>
            ) : null}
            {actionError ? (
              <p className="text-danger" role="alert">
                {actionError}
              </p>
            ) : null}
          </div>
        </>
      )}
    </div>
  );
}
