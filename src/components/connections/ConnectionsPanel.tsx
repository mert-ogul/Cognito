import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  getConnectionsMap,
  getConnectionsTimeline,
  getDocumentPreview,
  getEntitiesIndex,
  getEntityDetail,
  type ConnectionsMap,
  type ConnectionsMapPoint,
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

type Tab = "index" | "map" | "timeline";

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

const TOPIC_HUES = [200, 25, 145, 45, 330, 175, 85, 15];

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

export function mapPointOpacity(
  point: ConnectionsMapPoint,
  selectedDocIds: Set<string> | null,
): number {
  if (!selectedDocIds || selectedDocIds.size === 0) return 1;
  return selectedDocIds.has(point.documentId) ? 1 : 0.2;
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

function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

function pageLabel(pageStart: number | null | undefined, pageEnd: number | null | undefined): string | null {
  if (pageStart != null && pageEnd != null && pageEnd !== pageStart) {
    return `pp. ${pageStart}-${pageEnd}`;
  }
  if (pageStart != null) {
    return `p. ${pageStart}`;
  }
  return null;
}

function topicColor(topicId: string | null | undefined, index: number): string {
  if (!topicId) return "hsl(200 12% 55%)";
  const hue = TOPIC_HUES[index % TOPIC_HUES.length] ?? 200;
  return `hsl(${hue} 28% 48%)`;
}

function normalizePoints(points: ConnectionsMapPoint[]): {
  documentId: string;
  title: string;
  x: number;
  y: number;
  topicId: string | null;
}[] {
  if (points.length === 0) return [];
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const p of points) {
    minX = Math.min(minX, p.x);
    maxX = Math.max(maxX, p.x);
    minY = Math.min(minY, p.y);
    maxY = Math.max(maxY, p.y);
  }
  const dx = maxX - minX || 1;
  const dy = maxY - minY || 1;
  const pad = 8;
  const w = 100 - pad * 2;
  const h = 100 - pad * 2;
  return points.map((p) => ({
    documentId: p.documentId,
    title: p.title,
    topicId: p.topicId,
    x: pad + ((p.x - minX) / dx) * w,
    y: pad + ((p.y - minY) / dy) * h,
  }));
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
  const mapQ = useQuery({
    queryKey: ["connections-map"],
    queryFn: getConnectionsMap,
    enabled: tab === "map",
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

  const selectedDocIds = useMemo(() => {
    const docs = detailQ.data?.documents;
    if (!selectedEntityId || !docs) return null;
    return new Set(docs.map((d) => d.documentId));
  }, [selectedEntityId, detailQ.data]);

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
        <ModeButton label="Map" active={tab === "map"} onClick={() => setTab("map")} />
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
        {tab === "map" ? (
          <MapView
            data={mapQ.data ?? null}
            loading={mapQ.isLoading}
            selectedDocIds={selectedDocIds}
            onSelectDocument={setDocument}
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
            <p className="px-2 text-muted">Entity not found.</p>
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

function MapView({
  data,
  loading,
  selectedDocIds,
  onSelectDocument,
}: {
  data: ConnectionsMap | null;
  loading: boolean;
  selectedDocIds: Set<string> | null;
  onSelectDocument: (documentId: string) => void;
}) {
  const [hoverId, setHoverId] = useState<string | null>(null);

  const points = useMemo(() => normalizePoints(data?.points ?? []), [data]);
  const byId = useMemo(() => new Map(points.map((p) => [p.documentId, p])), [points]);
  const topicIndex = useMemo(() => {
    const m = new Map<string, number>();
    for (const t of data?.topics ?? []) {
      if (!m.has(t.id)) m.set(t.id, m.size);
    }
    for (const p of points) {
      if (p.topicId && !m.has(p.topicId)) m.set(p.topicId, m.size);
    }
    return m;
  }, [data, points]);

  const centroids = useMemo(() => {
    const acc = new Map<string, { sx: number; sy: number; n: number; name: string }>();
    const names = new Map((data?.topics ?? []).map((t) => [t.id, t.name]));
    for (const p of points) {
      if (!p.topicId) continue;
      const cur = acc.get(p.topicId) ?? {
        sx: 0,
        sy: 0,
        n: 0,
        name: names.get(p.topicId) ?? "",
      };
      cur.sx += p.x;
      cur.sy += p.y;
      cur.n += 1;
      acc.set(p.topicId, cur);
    }
    return [...acc.entries()].map(([id, v]) => ({
      id,
      name: v.name,
      x: v.sx / v.n,
      y: v.sy / v.n,
    }));
  }, [points, data]);

  if (loading) {
    return <p className="p-3 text-muted">Loading map…</p>;
  }
  if (!data || points.length === 0) {
    return <p className="p-3 text-muted">No document map yet.</p>;
  }

  const links = (data.links ?? []).filter((l) => l.kind === "mentions");

  return (
    <div className="relative h-full min-h-[220px] p-2">
      <svg viewBox="0 0 100 100" className="h-full w-full" role="img" aria-label="Connections map">
        {links.map((l) => {
          const a = byId.get(l.sourceId);
          const b = byId.get(l.targetId);
          if (!a || !b) return null;
          const fade =
            selectedDocIds &&
            selectedDocIds.size > 0 &&
            (!selectedDocIds.has(a.documentId) || !selectedDocIds.has(b.documentId));
          return (
            <line
              key={`${l.sourceId}-${l.targetId}-${l.evidenceChunkId ?? ""}`}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              stroke="currentColor"
              className="text-line"
              strokeWidth={0.3}
              opacity={fade ? 0.15 : 0.55}
            />
          );
        })}
        {centroids.map((c) =>
          c.name ? (
            <text
              key={`label-${c.id}`}
              x={c.x}
              y={c.y - 2.5}
              textAnchor="middle"
              className="fill-muted"
              fontSize={2.4}
            >
              {c.name}
            </text>
          ) : null,
        )}
        {points.map((p) => {
          const opacity = mapPointOpacity(p, selectedDocIds);
          const idx = p.topicId ? (topicIndex.get(p.topicId) ?? 0) : 0;
          return (
            <circle
              key={p.documentId}
              cx={p.x}
              cy={p.y}
              r={1.6}
              fill={topicColor(p.topicId, idx)}
              opacity={opacity}
              className="cursor-pointer"
              onMouseEnter={() => setHoverId(p.documentId)}
              onMouseLeave={() => setHoverId(null)}
              onClick={() => onSelectDocument(p.documentId)}
            >
              <title>{p.title || p.documentId}</title>
            </circle>
          );
        })}
      </svg>
      {hoverId ? (
        <p className="pointer-events-none absolute bottom-2 left-2 right-2 truncate text-[12px] text-muted">
          {byId.get(hoverId)?.title || hoverId}
        </p>
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
