import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { FIRST_IMPORT_DOC_CAP } from "@/components/library/constants";
import {
  getSettings,
  importDocuments,
  listDocuments,
  retryFailedImport,
  selectVaultFolder,
  setVault,
  type BackgroundProgress,
  type DocumentRow,
  type ImportProgress,
} from "@/lib/tauri";
import { useTauriEvent } from "@/lib/tauri-events";
import { cn } from "@/lib/utils";
import { fileName } from "@/lib/format";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";
import engravedDark from "@/assets/branding/cognito-engraved-dark.svg";
import engravedLight from "@/assets/branding/cognito-engraved-light.svg";

type SortKey = "title" | "type" | "date" | "pages";


function folderName(path: string | null | undefined): string {
  if (!path) return "";
  const trimmed = path.replace(/[/\\]+$/, "");
  const i = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return i >= 0 ? trimmed.slice(i + 1) : trimmed;
}

function displayTitle(d: DocumentRow): string {
  return d.title.trim() || fileName(d.path);
}

function cardTypeLabel(d: DocumentRow): string | null {
  const t = (d.cardType ?? "").trim();
  return t || null;
}

function peopleOrOrgs(d: DocumentRow): string[] {
  const people = d.cardPeople ?? [];
  const orgs = d.cardOrganizations ?? [];
  return [...people, ...orgs].filter((s) => s.trim().length > 0);
}

function scanStatusWords(status: string | null | undefined): string | null {
  if (status == null || status.trim() === "") return null;
  const s = status.trim().toLowerCase();
  if (
    s === "no_text" ||
    s === "empty" ||
    s === "no text found" ||
    s.includes("no_text") ||
    s.includes("no text")
  ) {
    return "No text found";
  }
  if (s === "partial_text" || s.includes("partial_text")) {
    return "Some pages have no text";
  }
  return "Scanned, text recognised";
}

function secondaryLine(d: DocumentRow): string {
  const parts: string[] = [];
  const type = cardTypeLabel(d);
  if (type) parts.push(type);
  if (d.cardDate) parts.push(d.cardDate);
  const names = peopleOrOrgs(d).slice(0, 2);
  if (names.length) parts.push(names.join(", "));
  if (d.pageCount != null) parts.push(`${d.pageCount} pages`);
  const scan = scanStatusWords(d.scanStatus);
  if (scan) parts.push(scan);
  return parts.join(" · ");
}

function sortValue(d: DocumentRow, key: SortKey): string | number {
  if (key === "title") return displayTitle(d).toLowerCase();
  if (key === "type") return (cardTypeLabel(d) ?? d.fileType ?? "").toLowerCase();
  if (key === "date") return d.cardDate ?? d.indexedAt ?? d.importedAt ?? "";
  return d.pageCount ?? -1;
}

function statusWord(d: DocumentRow, summariseOn: boolean): string | null {
  if (d.status === "pending" || d.status === "parsing" || d.status === "embedding") {
    return "Indexing";
  }
  if (d.status === "failed" || !!d.errorMessage) {
    return "Failed";
  }
  if (
    summariseOn &&
    d.status === "indexed" &&
    (d.cardStatus === "pending" || d.cardStatus === "running")
  ) {
    return "Summarising";
  }
  return null;
}

export function LibraryPanel() {
  const qc = useQueryClient();
  const setDocument = useMainViewStore((s) => s.setDocument);
  const selectedId = useMainViewStore((s) => s.documentId);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [backgroundProgress, setBackgroundProgress] = useState<BackgroundProgress | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("title");
  const [typeFilter, setTypeFilter] = useState<string>("");
  const [search, setSearch] = useState("");

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
      void qc.invalidateQueries();
      useChatIntentStore.getState().bumpLibraryReset();
      useMainViewStore.getState().showConversation();
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

  useTauriEvent<ImportProgress>("import-progress", (ev) => {
    setProgress(ev.payload);
    if (ev.payload.phase === "done" || ev.payload.status === "completed") {
      setBackgroundProgress(null);
    }
    invalidateDocs();
  });

  useTauriEvent<BackgroundProgress>("background-progress", (ev) => {
    setBackgroundProgress(ev.payload);
    invalidateDocs();
  });

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

  const allRows = docs.data ?? [];
  const typeOptions = useMemo(() => {
    const set = new Set<string>();
    for (const d of allRows) {
      const t = cardTypeLabel(d);
      if (t) set.add(t);
    }
    return [...set].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  }, [allRows]);

  const rows = useMemo(() => {
    const q = search.trim().toLowerCase();
    let list = [...allRows];
    if (typeFilter) {
      list = list.filter((d) => cardTypeLabel(d) === typeFilter);
    }
    if (q) {
      list = list.filter((d) => {
        const title = displayTitle(d).toLowerCase();
        const people = (d.cardPeople ?? []).join(" ").toLowerCase();
        const orgs = (d.cardOrganizations ?? []).join(" ").toLowerCase();
        return title.includes(q) || people.includes(q) || orgs.includes(q);
      });
    }
    list.sort((a, b) => {
      const av = sortValue(a, sortKey);
      const bv = sortValue(b, sortKey);
      if (typeof av === "number" && typeof bv === "number") {
        return av - bv;
      }
      return String(av).localeCompare(String(bv), undefined, { sensitivity: "base" });
    });
    return list;
  }, [allRows, search, sortKey, typeFilter]);

  const vaultPath = settings.data?.vaultPath ?? null;
  const vaultSet = !!vaultPath;
  const underCap = allRows.length < FIRST_IMPORT_DOC_CAP;
  const failedCount = allRows.filter((d) => d.status === "failed" || d.errorMessage).length;
  const entitiesWaiting = useMemo(
    () =>
      allRows.filter(
        (d) =>
          d.status === "indexed" &&
          (d.entitiesStatus === "pending" || d.entitiesStatus === "running"),
      ).length,
    [allRows],
  );
  const extractOn = settings.data?.extractEntities !== false;
  const summariseOn = settings.data?.summariseDocuments !== false;
  const indexed = useMemo(
    () => allRows.filter((d) => d.status === "indexed"),
    [allRows],
  );
  const cardOk = indexed.filter((d) => d.cardStatus === "ok").length;
  const cardsBusy = indexed.some(
    (d) => d.cardStatus === "pending" || d.cardStatus === "running",
  );
  const importActive =
    !!progress &&
    progress.totalFiles > 0 &&
    progress.status !== "completed" &&
    progress.phase !== "done";
  const importPct = importActive
    ? Math.round((100 * progress.completedFiles) / progress.totalFiles)
    : 0;

  const sortOptions: { key: SortKey; label: string }[] = [
    { key: "title", label: "title" },
    { key: "type", label: "type" },
    { key: "date", label: "date" },
    { key: "pages", label: "pages" },
  ];

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
          <picture>
            <source media="(prefers-color-scheme: dark)" srcSet={engravedDark} />
            <img
              src={engravedLight}
              alt=""
              aria-hidden
              width={96}
              className="select-none"
              draggable={false}
            />
          </picture>
          <p className="text-muted">
            Pick a folder of PDF, Word and Markdown files. Cognito indexes it on this computer.
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
          <div className="shrink-0 space-y-1.5 border-b border-line px-3 py-2">
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search title, people, orgs"
              aria-label="Search library"
            />
            <p className="text-muted">
              Sort{" "}
              {sortOptions.map((opt, i) => (
                <span key={opt.key}>
                  {i > 0 ? " | " : null}
                  <button
                    type="button"
                    className={cn(
                      "hover:text-text",
                      sortKey === opt.key ? "text-text" : "text-muted",
                    )}
                    onClick={() => setSortKey(opt.key)}
                  >
                    {opt.label}
                  </button>
                </span>
              ))}
            </p>
            {typeOptions.length > 0 ? (
              <p className="text-muted">
                Type{" "}
                <button
                  type="button"
                  className={cn("hover:text-text", !typeFilter ? "text-text" : "text-muted")}
                  onClick={() => setTypeFilter("")}
                >
                  all
                </button>
                {typeOptions.map((t) => (
                  <span key={t}>
                    {" | "}
                    <button
                      type="button"
                      className={cn(
                        "hover:text-text",
                        typeFilter === t ? "text-text" : "text-muted",
                      )}
                      onClick={() => setTypeFilter(t)}
                    >
                      {t}
                    </button>
                  </span>
                ))}
              </p>
            ) : null}
          </div>

          <ul className="min-h-0 flex-1 overflow-auto">
            {rows.map((d) => {
              const word = statusWord(d, summariseOn);
              const failed = word === "Failed";
              const line = secondaryLine(d);
              const selected = selectedId === d.id;
              return (
                <li
                  key={d.id}
                  className={cn(
                    "border-b border-line px-3 py-2 hover:bg-hover",
                    selected ? "bg-hover" : "",
                  )}
                >
                  <button
                    type="button"
                    className="w-full text-left"
                    onClick={() => setDocument(d.id)}
                  >
                    <p className="truncate text-text" title={d.path}>
                      {displayTitle(d)}
                    </p>
                    {line ? <p className="truncate text-muted">{line}</p> : null}
                    {word && !failed ? <p className="text-muted">{word}</p> : null}
                  </button>
                  {failed ? (
                    <div className="mt-0.5 flex flex-wrap items-center gap-2">
                      <p className="text-danger" title={d.errorMessage ?? undefined}>
                        Failed
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
                  {d.entitiesStatus === "failed" ? (
                    <p className="text-danger" title={d.entitiesError ?? undefined}>
                      Names failed
                    </p>
                  ) : null}
                  {d.entitiesStatus === "ok" && d.entitiesError ? (
                    <p className="text-muted">{d.entitiesError}</p>
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
            {summariseOn && (cardsBusy || backgroundProgress) && indexed.length > 0 ? (
              <p className="text-muted">
                Summarising {cardOk} of {indexed.length}
              </p>
            ) : null}
            {extractOn && entitiesWaiting > 0 ? (
              <p className="text-muted">
                Finding names, {entitiesWaiting} file{entitiesWaiting === 1 ? "" : "s"} left
              </p>
            ) : null}
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
