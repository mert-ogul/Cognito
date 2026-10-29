import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { getGraphSnapshot, getSettings, searchGraphNodes, type GraphSnapshot } from "@/lib/tauri";
import { useSidePaneStore } from "@/stores/sidePaneStore";
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
  const highlightNodeIds = useSidePaneStore((s) => s.highlightNodeIds);

  const settingsQ = useQuery({ queryKey: ["settings"], queryFn: getSettings });
  const graphEnabled = settingsQ.data?.graphExtractionEnabled !== false;

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

  const emptyMessage = graphEnabled
    ? "The graph fills in as documents are indexed."
    : "Graph building is off. You can turn it on in Settings.";

  const openNode = useCallback((id: string) => {
    setSelectedNodeId(id);
    setSheetOpen(true);
    setCenterOn(id);
  }, []);

  const onNodeTap = useCallback(
    (id: string) => {
      openNode(id);
    },
    [openNode],
  );

  const onPickSearch = useCallback(
    (id: string) => {
      setSearch("");
      openNode(id);
    },
    [openNode],
  );

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <div className="shrink-0 space-y-2 border-b border-line px-3 py-2">
        <div className="flex flex-wrap gap-2">
          <Input
            placeholder="Search nodes…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search graph nodes by name"
            className="min-w-[140px] flex-1"
          />
          <Button type="button" variant="quiet" onClick={() => setFitKey((k) => k + 1)}>
            Fit
          </Button>
          {truncated && limitIdx < SNAPSHOT_STEPS.length - 1 ? (
            <Button type="button" variant="quiet" onClick={() => setLimitIdx((i) => i + 1)}>
              Load more ({SNAPSHOT_STEPS[limitIdx + 1]})
            </Button>
          ) : null}
        </div>
        {debouncedSearch.trim().length >= 2 && searchQ.data && searchQ.data.length > 0 ? (
          <ul
            className="max-h-28 overflow-y-auto border border-line bg-bg"
            role="listbox"
            aria-label="Search results"
          >
            {searchQ.data.map((h) => (
              <li key={h.nodeId}>
                <button
                  type="button"
                  className="flex w-full items-center justify-between px-2 py-1.5 text-left hover:bg-hover"
                  onClick={() => onPickSearch(h.nodeId)}
                >
                  <span className="font-medium text-text">{h.name}</span>
                  <span className="text-muted">{h.kind}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      <div className="relative flex min-h-0 flex-1 flex-col">
        {snapQ.isLoading ? <p className="p-3 text-muted">Loading graph…</p> : null}
        {snapQ.isError ? (
          <div className="flex flex-wrap items-center gap-2 p-3" role="alert">
            <p className="text-danger">Couldn&apos;t load the graph</p>
            <Button type="button" variant="quiet" onClick={() => void snapQ.refetch()}>
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
          emptyMessage={emptyMessage}
        />
        <GraphNodeDetailSheet
          nodeId={sheetOpen ? selectedNodeId : null}
          open={sheetOpen}
          onClose={() => {
            setSheetOpen(false);
            setSelectedNodeId(null);
          }}
        />
      </div>
    </div>
  );
}
