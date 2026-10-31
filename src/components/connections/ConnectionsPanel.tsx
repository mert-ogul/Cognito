import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  getConnectionsTimeline,
  getDocumentPreview,
  getEntitiesIndex,
  getEntityDetail,
  type EntityDetail,
  type EntityIndexItem,
  type TimelineItem,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useChatIntentStore } from "@/stores/chatIntentStore";
import { useCitedRegionsStore } from "@/stores/citedRegionsStore";
import { useMainViewStore } from "@/stores/mainViewStore";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { fileName, pageLabel } from "@/lib/format";

type Tab = "index" | "timeline";

const PRIMARY_FILTERS: { key: string; label: string }[] = [
  { key: "all", label: "All" },
  { key: "person", label: "People" },
  { key: "organization", label: "Organizations" },
  { key: "place", label: "Places" },
  { key: "date", label: "Dates" },
];

const PRIMARY_KEYS = new Set(PRIMARY_FILTERS.map((f) => f.key).filter((k) => k !== "all"));

const LABEL_DISPLAY: Record<string, string> = {
  person: "People",
  organization: "Organizations",
  place: "Places",
  date: "Dates",
};

export function labelTabName(label: string): string {
  const key = label.trim().toLowerCase();
  return LABEL_DISPLAY[key] ?? (label.trim() || "Other");
}

export function filterAndSortEntities(
  items: EntityIndexItem[],
  labelFilter: string,
  search: string,
): EntityIndexItem[] {
  const q = search.trim().toLowerCase();
  const filtered = items.filter((e) => {
    if (labelFilter !== "all" && e.label.trim().toLowerCase() !== labelFilter) {
      return false;
    }
    if (q && !e.name.toLowerCase().includes(q)) {
      return false;
    }
    return true;
  });
  return [...filtered].sort((a, b) => {
    if (b.docCount !== a.docCount) return b.docCount - a.docCount;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });
}

export function groupTimelineByYear(
  items: TimelineItem[],
): { year: string; items: TimelineItem[] }[] {
  const byYear = new Map<string, TimelineItem[]>();
  for (const item of items) {
    const year = timelineYear(item.date);
    const list = byYear.get(year) ?? [];
    list.push(item);
    byYear.set(year, list);
  }
  return [...byYear.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([year, group]) => ({
      year,
      items: [...group].sort((a, b) => a.date.localeCompare(b.date)),
    }));
}

function timelineYear(date: string): string {
  const m = date.trim().match(/^(\d{4})/);
  return m ? m[1] : "Unknown";
}



export function ConnectionsPanel() {
  const [tab, setTab] = useState<Tab>("index");
  const [labelFilter, setLabelFilter] = useState("all");
  const [search, setSearch] = useState("");
  const selectedEntityId = useSidePaneStore((s) => s.selectedEntityId);
  const setSelectedEntityId = useSidePaneStore((s) => s.setSelectedEntityId);
  const openPassage = useSidePaneStore((s) => s.openPassage);
  const addCitedRegions = useCitedRegionsStore((s) => s.addRegions);
  const setDocument = useMainViewStore((s) => s.setDocument);
  const showConversation = useMainViewStore((s) => s.showConversation);
  const setRelatedAsk = useChatIntentStore((s) => s.setRelatedAsk);

  const indexQ = useQuery({
    queryKey: ["entities-index"],
    queryFn: getEntitiesIndex,
  });
  const detailQ = useQuery({
    queryKey: ["entity-detail", selectedEntityId],
    queryFn: () => getEntityDetail(selectedEntityId!),
    enabled: !!selectedEntityId,
  });
  const timelineQ = useQuery({
    queryKey: ["connections-timeline"],
    queryFn: getConnectionsTimeline,
    enabled: tab === "timeline",
  });

  const entities = indexQ.data ?? [];
  const extraLabels = useMemo(() => {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const e of entities) {
      const key = e.label.trim().toLowerCase();
      if (!key || PRIMARY_KEYS.has(key) || seen.has(key)) continue;
      seen.add(key);
      out.push(key);
    }
    return out.sort();
  }, [entities]);

  const sorted = useMemo(
    () => filterAndSortEntities(entities, labelFilter, search),
    [entities, labelFilter, search],
  );

  const openEntityPassage = async (
    documentId: string,
    title: string,
    path: string,
    chunkId: string,
    snippet: string,
    pageStart: number | null,
    pageEnd: number | null,
  ) => {
    const prev = await getDocumentPreview(documentId, chunkId);
    const start = prev.pageStart ?? pageStart;
    const end = prev.pageEnd ?? pageEnd;
    const regions = prev.regions ?? null;
    const resolvedPath = prev.path || path;
    addCitedRegions(documentId, regions);
    openPassage({
      documentId,
      title: title.trim() || fileName(path),
      pageLabel: pageLabel(start, end),
      text: prev.text,
      highlight: prev.highlightSnippet ?? snippet,
      path: resolvedPath,
      page: start ?? regions?.[0]?.page ?? undefined,
      pageCount: null,
      regions,
    });
  };

  const askRelated = (other: { entityId: string; name: string }, detail: EntityDetail) => {
    setRelatedAsk({
      entityIdA: detail.id,
      entityIdB: other.entityId,
      nameA: detail.name,
      nameB: other.name,
    });
    showConversation();
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 gap-1 border-b border-line px-2 py-1">
        <ModeButton label="Index" active={tab === "index"} onClick={() => setTab("index")} />
        <ModeButton
          label="Timeline"
          active={tab === "timeline"}
          onClick={() => setTab("timeline")}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {tab === "index" ? (
          <IndexView
            labelFilter={labelFilter}
            setLabelFilter={setLabelFilter}
            search={search}
            setSearch={setSearch}
            extraLabels={extraLabels}
            sorted={sorted}
            selectedEntityId={selectedEntityId}
            onSelect={setSelectedEntityId}
            detail={detailQ.data ?? null}
            detailLoading={detailQ.isLoading}
            onOpenPassage={openEntityPassage}
            onAskRelated={askRelated}
            onOpenDocument={setDocument}
          />
        ) : null}
        {tab === "timeline" ? (
          <TimelineView
            items={timelineQ.data?.items ?? []}
            loading={timelineQ.isLoading}
            onSelectDocument={setDocument}
            onOpenEntity={async (entityId) => {
              setSelectedEntityId(entityId);
              setTab("index");
              const detail = await getEntityDetail(entityId);
              const first = detail?.documents[0]?.passages[0];
              const doc = detail?.documents[0];
              if (detail && doc && first) {
                await openEntityPassage(
                  doc.documentId,
                  doc.title,
                  doc.path,
                  first.chunkId,
                  first.snippet,
                  first.pageStart,
                  first.pageEnd,
                );
              }
            }}
          />
        ) : null}
      </div>
    </div>
  );
}

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

function IndexView({
  labelFilter,
  setLabelFilter,
  search,
  setSearch,
  extraLabels,
  sorted,
  selectedEntityId,
  onSelect,
  detail,
  detailLoading,
  onOpenPassage,
  onAskRelated,
  onOpenDocument,
}: {
  labelFilter: string;
  setLabelFilter: (v: string) => void;
  search: string;
  setSearch: (v: string) => void;
  extraLabels: string[];
  sorted: EntityIndexItem[];
  selectedEntityId: string | null;
  onSelect: (id: string | null) => void;
  detail: EntityDetail | null;
  detailLoading: boolean;
  onOpenPassage: (
    documentId: string,
    title: string,
    path: string,
    chunkId: string,
    snippet: string,
    pageStart: number | null,
    pageEnd: number | null,
  ) => Promise<void>;
  onAskRelated: (other: { entityId: string; name: string }, detail: EntityDetail) => void;
  onOpenDocument: (documentId: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2 p-2">
      <div className="flex flex-wrap gap-1">
        {PRIMARY_FILTERS.map((f) => (
          <button
            key={f.key}
            type="button"
            className={cn(
              "px-1.5 py-0.5 text-[12px]",
              labelFilter === f.key ? "text-text" : "text-muted hover:text-text",
            )}
            aria-pressed={labelFilter === f.key}
            onClick={() => setLabelFilter(f.key)}
          >
            {f.label}
          </button>
        ))}
        {extraLabels.map((lab) => (
          <button
            key={lab}
            type="button"
            className={cn(
              "px-1.5 py-0.5 text-[12px]",
              labelFilter === lab ? "text-text" : "text-muted hover:text-text",
            )}
            aria-pressed={labelFilter === lab}
            onClick={() => setLabelFilter(lab)}
          >
            {labelTabName(lab)}
          </button>
        ))}
      </div>
      <Input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search names"
        className="h-8"
      />
      <ul className="flex flex-col gap-0.5">
        {sorted.map((e) => (
          <li key={e.id}>
            <button
              type="button"
              className={cn(
                "w-full rounded-none px-2 py-1.5 text-left",
                selectedEntityId === e.id ? "bg-line/60" : "hover:bg-line/40",
              )}
              onClick={() => onSelect(e.id === selectedEntityId ? null : e.id)}
            >
              <div className="text-[13px] text-text">{e.name}</div>
              <div className="text-[12px] text-muted">
                {e.docCount} documents, {e.mentionCount} passages
              </div>
            </button>
          </li>
        ))}
      </ul>
      {selectedEntityId ? (
        <div className="mt-2 border-t border-line pt-2">
          {detailLoading ? (
            <p className="px-2 text-muted">Loading…</p>
          ) : detail ? (
            <EntityDetailView
              detail={detail}
              onOpenPassage={onOpenPassage}
              onAskRelated={onAskRelated}
              onOpenDocument={onOpenDocument}
            />
          ) : (
            <p className="px-2 text-muted">This name is no longer in your documents.</p>
          )}
        </div>
      ) : null}
    </div>
  );
}

function EntityDetailView({
  detail,
  onOpenPassage,
  onAskRelated,
  onOpenDocument,
}: {
  detail: EntityDetail;
  onOpenPassage: (
    documentId: string,
    title: string,
    path: string,
    chunkId: string,
    snippet: string,
    pageStart: number | null,
    pageEnd: number | null,
  ) => Promise<void>;
  onAskRelated: (other: { entityId: string; name: string }, detail: EntityDetail) => void;
  onOpenDocument: (documentId: string) => void;
}) {
  return (
    <div className="flex flex-col gap-3 px-2 pb-3">
      <div>
        <p className="font-medium text-text">{detail.name}</p>
        <p className="text-[12px] text-muted">{labelTabName(detail.label)}</p>
      </div>
      <div>
        <p className="mb-1 text-[12px] text-muted">Documents</p>
        <ul className="flex flex-col gap-2">
          {detail.documents.map((doc) => (
            <li key={doc.documentId}>
              <button
                type="button"
                className="text-left text-[13px] text-text hover:underline"
                onClick={() => onOpenDocument(doc.documentId)}
              >
                {doc.title.trim() || fileName(doc.path)}
              </button>
              <ul className="mt-0.5 flex flex-col gap-0.5 pl-2">
                {doc.passages.map((p) => (
                  <li key={`${doc.documentId}-${p.chunkId}-${p.snippet.slice(0, 24)}`}>
                    <button
                      type="button"
                      className="text-left text-[12px] text-muted hover:text-text"
                      onClick={() =>
                        void onOpenPassage(
                          doc.documentId,
                          doc.title,
                          doc.path,
                          p.chunkId,
                          p.snippet,
                          p.pageStart,
                          p.pageEnd,
                        )
                      }
                    >
                      {p.snippet || "Open passage"}
                    </button>
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      </div>
      {detail.coMentions.length > 0 ? (
        <div>
          <p className="mb-1 text-[12px] text-muted">Often mentioned with</p>
          <ul className="flex flex-col gap-1.5">
            {detail.coMentions.map((c) => (
              <li key={c.entityId} className="flex flex-col gap-0.5">
                <span className="text-[13px] text-text">
                  {c.name}{" "}
                  <span className="text-muted">({c.count})</span>
                </span>
                <button
                  type="button"
                  className="w-fit text-[12px] text-accent hover:underline"
                  onClick={() => onAskRelated({ entityId: c.entityId, name: c.name }, detail)}
                >
                  How are they related?
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function TimelineView({
  items,
  loading,
  onSelectDocument,
  onOpenEntity,
}: {
  items: TimelineItem[];
  loading: boolean;
  onSelectDocument: (documentId: string) => void;
  onOpenEntity: (entityId: string) => void;
}) {
  const groups = useMemo(() => groupTimelineByYear(items), [items]);

  if (loading) {
    return <p className="p-3 text-muted">Loading timeline…</p>;
  }
  if (groups.length === 0) {
    return <p className="p-3 text-muted">No dated documents or dates yet.</p>;
  }

  return (
    <div className="flex flex-col gap-4 overflow-x-auto p-3">
      <div className="flex min-w-max gap-6">
        {groups.map((g) => (
          <div key={g.year} className="w-[160px] shrink-0">
            <p className="mb-2 border-b border-line pb-1 text-[12px] text-muted">{g.year}</p>
            <ul className="flex flex-col gap-1.5">
              {g.items.map((item) => {
                const label =
                  item.kind === "document"
                    ? item.title || item.id
                    : item.name || item.id;
                return (
                  <li key={`${item.kind}-${item.id}-${item.date}`}>
                    <button
                      type="button"
                      className="w-full text-left text-[13px] text-text hover:underline"
                      onClick={() => {
                        if (item.kind === "document") {
                          onSelectDocument(item.documentId ?? item.id);
                        } else {
                          onOpenEntity(item.entityId ?? item.id);
                        }
                      }}
                    >
                      <span className="block text-[11px] text-muted">{item.date}</span>
                      {label}
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </div>
    </div>
  );
}
