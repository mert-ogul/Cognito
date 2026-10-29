import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { getGraphSnapshot, searchGraphNodes, type GraphSnapshot } from "@/lib/tauri";
import { useGraphSessionStore } from "@/stores/graphSessionStore";
import { useQuery } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";
import { GraphCyCanvas } from "./GraphCyCanvas";
import { GraphNodeDetailSheet } from "./GraphNodeDetailSheet";

type SearchHit = { nodeId: string; name: string; kind: string; sourceCount?: number };

const SNAPSHOT_STEPS = [200, 400] as const;

export function GraphPanel() {
  const [limitIdx, setLimitIdx] = useState(0);
  const limit = SNAPSHOT_STEPS[limitIdx] ?? 200;
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search, 280);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [sheetOpen, setSheetOpen] = useState(false);
  const [fitKey, setFitKey] = useState(0);
  const [centerOn, setCenterOn] = useState<string | null>(null);
  const highlightNodeIds = useGraphSessionStore((s) => s.highlightNodeIds);

  const snapQ = useQuery({
    queryKey: ["graph-snapshot", limit],
    queryFn: () => getGraphSnapshot(limit),
  });

  const searchQ = useQuery({
    queryKey: ["graph-search", debouncedSearch],
    queryFn: () => searchGraphNodes(debouncedSearch.trim(), 24) as Promise<SearchHit[]>,
    enabled: debouncedSearch.trim().length >= 2,
  });

  const snapshot = snapQ.data ?? null;
  const truncated = useMemo(() => {
    if (!snapshot?.nodes?.length) return false;
    return snapshot.nodes.length >= limit;
  }, [snapshot, limit]);

  const openNode = useCallback((id: string) => {
    setSelectedNodeId(id);
    setSheetOpen(true);
    setCenterOn(id);
  }, []);

  const onNodeTap = useCallback((id: string) => {
    openNode(id);
  }, [openNode]);

  const onPickSearch = useCallback(
    (id: string) => {
      setSearch("");
      openNode(id);
    },
    [openNode],
  );

  return (
    <Card className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <CardHeader className="shrink-0 space-y-2 pb-2">
        <div className="flex items-start justify-between gap-2">
          <CardTitle>Graph</CardTitle>
          <span className="rounded-full bg-[var(--background)] px-2 py-0.5 font-mono text-[10px] text-[var(--muted)]">
            read-only
          </span>
        </div>
        <p className="text-[10px] text-[var(--muted)]">
          Top nodes by salience, capped edges. Pan and zoom, no editing.
        </p>
        <div className="flex flex-wrap gap-2">
          <Input
            placeholder="Search node by name…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search graph nodes by name"
            className="h-8 min-w-[140px] flex-1 text-xs"
          />
          <Button type="button" variant="outline" size="sm" onClick={() => setFitKey((k) => k + 1)}>
            Fit
          </Button>
          {truncated && limitIdx < SNAPSHOT_STEPS.length - 1 ? (
            <Button type="button" variant="ghost" size="sm" onClick={() => setLimitIdx((i) => i + 1)}>
              Load more ({SNAPSHOT_STEPS[limitIdx + 1]})
            </Button>
          ) : null}
        </div>
        {debouncedSearch.trim().length >= 2 && searchQ.data && searchQ.data.length > 0 ? (
          <ul
            className="max-h-28 overflow-y-auto rounded-md border border-[var(--border)] bg-[var(--surface)] text-xs"
            role="listbox"
            aria-label="Search results"
          >
            {searchQ.data.map((h) => (
              <li key={h.nodeId}>
                <button
                  type="button"
                  className="flex w-full items-center justify-between px-2 py-1.5 text-left hover:bg-[var(--background)]"
                  onClick={() => onPickSearch(h.nodeId)}
                >
                  <span className="font-medium">{h.name}</span>
                  <span className="text-[10px] text-[var(--muted)]">{h.kind}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-2">
        {snapQ.isLoading ? <p className="text-xs text-[var(--muted)]">Loading graph…</p> : null}
        {snapQ.isError ? (
          <div className="flex flex-wrap items-center gap-2" role="alert">
            <p className="text-xs text-red-500">Couldn&apos;t load the graph</p>
            <Button type="button" size="sm" variant="outline" onClick={() => void snapQ.refetch()}>
              Retry
            </Button>
          </div>
        ) : null}
        <GraphCyCanvas
          snapshot={snapshot as GraphSnapshot | null}
          highlightNodeIds={highlightNodeIds}
          selectedNodeId={selectedNodeId}
          onNodeTap={onNodeTap}
          fitKey={fitKey}
          centerOnNodeId={centerOn}
        />
        {truncated ? (
          <p className="text-[10px] text-[var(--muted)]">Showing up to {limit} nodes (salience-ranked).</p>
        ) : null}
      </CardContent>
      <GraphNodeDetailSheet
        nodeId={sheetOpen ? selectedNodeId : null}
        open={sheetOpen}
        onClose={() => {
          setSheetOpen(false);
          setSelectedNodeId(null);
        }}
      />
    </Card>
  );
}
