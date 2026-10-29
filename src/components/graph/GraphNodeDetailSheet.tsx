import { Button } from "@/components/ui/button";
import { getDocumentPreview, getGraphNodeDetail, listDocuments, type DocumentRow } from "@/lib/tauri";
import { useSidePaneStore } from "@/stores/sidePaneStore";
import { useQuery } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";

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
  sourceName?: string;
  targetName?: string;
};

type DetailPayload = { node: NodeJson; edges: EdgeJson[] };

type Props = {
  nodeId: string | null;
  open: boolean;
  onClose: () => void;
};

function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

export function GraphNodeDetailSheet({ nodeId, open, onClose }: Props) {
  const openPassage = useSidePaneStore((s) => s.openPassage);
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

  const openEdgePassage = useCallback(
    async (e: EdgeJson) => {
      const prev = await getDocumentPreview(e.documentId, e.chunkId);
      const doc = (docsQ.data ?? []).find((d) => d.id === e.documentId);
      const title =
        (doc?.title || "").trim() ||
        (doc?.path ? fileName(doc.path) : e.documentId);
      const start = prev.pageStart;
      const end = prev.pageEnd;
      let pageLabel: string | null = null;
      if (start != null && end != null && end !== start) {
        pageLabel = `pp. ${start}-${end}`;
      } else if (start != null) {
        pageLabel = `p. ${start}`;
      }
      openPassage({
        documentId: prev.documentId,
        title,
        pageLabel,
        text: prev.text,
        highlight: prev.highlightSnippet ?? null,
      });
    },
    [docsQ.data, openPassage],
  );

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
      className="absolute right-0 top-0 z-20 flex h-full w-[min(100%,320px)] flex-col border-l border-line bg-pane"
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
            Could not load this node.
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
                  const src = e.sourceName || e.sourceNodeId;
                  const tgt = e.targetName || e.targetNodeId;
                  const line = `${src}, ${e.relation}, ${tgt}`;
                  return (
                    <li key={e.edgeId} className="text-text">
                      <span>{line}</span>{" "}
                      {e.chunkId ? (
                        <button
                          type="button"
                          className="text-accent underline"
                          onClick={() => void openEdgePassage(e)}
                        >
                          Source
                        </button>
                      ) : null}
                    </li>
                  );
                })}
                {edges.length > 40 ? <li className="text-muted">…and more</li> : null}
              </ul>
            </div>
            <div>
              <p className="text-muted">Supporting documents</p>
              {supporting.length === 0 ? (
                <p className="text-muted">No matching library titles yet.</p>
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
