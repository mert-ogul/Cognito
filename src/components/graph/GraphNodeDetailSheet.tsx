import { Button } from "@/components/ui/button";
import { getGraphNodeDetail, listDocuments, type DocumentRow } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

type NodeJson = {
  nodeId: string;
  name: string;
  kind: string;
  sourceCount: number;
};

type EdgeJson = {
  edgeId: string;
  sourceNodeId: string;
  targetNodeId: string;
  relation: string;
  documentId: string;
  chunkId: string;
  confidence: number;
};

type DetailPayload = { node: NodeJson; edges: EdgeJson[] };

type Props = {
  nodeId: string | null;
  open: boolean;
  onClose: () => void;
};

export function GraphNodeDetailSheet({ nodeId, open, onClose }: Props) {
  const detailQ = useQuery({
    queryKey: ["graph-node-detail", nodeId],
    queryFn: () => getGraphNodeDetail(nodeId!) as Promise<DetailPayload | null>,
    enabled: open && !!nodeId,
  });

  const docsQ = useQuery({ queryKey: ["documents"], queryFn: listDocuments });

  const supporting = useMemo(() => {
    const edges = detailQ.data?.edges;
    if (!edges?.length) return [];
    const ids = [...new Set(edges.map((e) => e.documentId))];
    const rows: DocumentRow[] = docsQ.data ?? [];
    const map = new Map(rows.map((d) => [d.id, d]));
    return ids.map((id) => map.get(id)).filter(Boolean) as DocumentRow[];
  }, [detailQ.data?.edges, docsQ.data]);

  if (!open || !nodeId) {
    return null;
  }

  const node = detailQ.data?.node;
  const edges = detailQ.data?.edges ?? [];

  return (
    <div
      role="dialog"
      aria-modal
      aria-labelledby="graph-node-detail-title"
      className={cn(
        "absolute right-0 top-0 z-20 flex h-full w-[min(100%,320px)] flex-col border-l border-line bg-bg",
      )}
    >
      <div className="flex items-center justify-between border-b border-line px-3 py-2">
        <h2 id="graph-node-detail-title" className="font-medium text-text">
          Node detail
        </h2>
        <Button type="button" variant="quiet" onClick={onClose}>
          Close
        </Button>
      </div>
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
        {detailQ.isLoading ? <p className="text-muted">Loading…</p> : null}
        {detailQ.isError ? (
          <p className="text-danger" role="alert">
            Could not load node detail.
          </p>
        ) : null}
        {node ? (
          <>
            <div>
              <p className="text-muted">Name</p>
              <p className="font-medium text-text">{node.name}</p>
            </div>
            <div>
              <p className="text-muted">Kind</p>
              <p className="text-text">{node.kind}</p>
            </div>
            <div>
              <p className="text-muted">Evidence (sources)</p>
              <p className="text-text">{node.sourceCount}</p>
            </div>
            <div>
              <p className="text-muted">Relations ({edges.length})</p>
              <ul className="mt-1 space-y-1">
                {edges.slice(0, 40).map((e) => {
                  const other =
                    e.sourceNodeId === node.nodeId ? e.targetNodeId : e.sourceNodeId;
                  return (
                    <li key={e.edgeId} className="border border-line px-2 py-1">
                      <span className="text-muted">{e.relation}</span> ·{" "}
                      <span className="font-mono text-[12px]" title={other}>
                        {shortId(other)}
                      </span>
                    </li>
                  );
                })}
                {edges.length > 40 ? <li className="text-muted">…and more</li> : null}
              </ul>
            </div>
            <div>
              <p className="text-muted">Supporting documents</p>
              {supporting.length === 0 ? (
                <p className="text-muted">No matching library titles (ids only in graph).</p>
              ) : (
                <ul className="mt-1 space-y-1">
                  {supporting.map((d) => (
                    <li key={d.id} className="truncate text-text" title={d.path}>
                      {d.title || d.path}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}

function shortId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 6)}…${id.slice(-4)}` : id;
}
