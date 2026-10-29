"""JSON-RPC: graph store and DuckDB purge."""

from __future__ import annotations

from typing import Any

from extraction.extraction_result import ExtractedEdge, ExtractedEntity
from extraction.graph_store import open_connection, purge_document_graph, upsert_entities_edges
from extraction.node_normalizer import normalize_name


def rpc_store_graph_chunk(params: dict[str, Any]) -> dict[str, Any]:
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    chunk_id = str(params["chunkId"])
    entities_raw = params.get("entities") or []
    relations_raw = params.get("relations") or []
    if not isinstance(entities_raw, list):
        entities_raw = []
    if not isinstance(relations_raw, list):
        relations_raw = []

    entities: list[ExtractedEntity] = []
    seen: set[str] = set()
    for item in entities_raw:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "").strip()
        kind = str(item.get("kind") or "concept").strip()[:64] or "concept"
        norm = normalize_name(name)
        if len(norm) < 2 or norm in seen:
            continue
        seen.add(norm)
        entities.append(
            ExtractedEntity(
                display_name=name[:512],
                kind=kind,
                normalized_name=norm,
                confidence=0.8,
            )
        )

    edges: list[ExtractedEdge] = []
    for item in relations_raw:
        if not isinstance(item, dict):
            continue
        sn = normalize_name(str(item.get("source") or ""))
        tn = normalize_name(str(item.get("target") or ""))
        rel = str(item.get("relation") or "").strip()[:256] or "related"
        if len(sn) < 2 or len(tn) < 2:
            continue
        if sn not in seen or tn not in seen:
            continue
        edges.append(
            ExtractedEdge(
                source_normalized=sn,
                target_normalized=tn,
                relation=rel,
                confidence=0.8,
            )
        )

    con = open_connection(duck_path)
    try:
        upsert_entities_edges(con, document_id, chunk_id, entities, edges)
        con.commit()
    finally:
        con.close()
    return {"ok": True, "entities": len(entities), "edges": len(edges)}


def rpc_purge_graph_document(params: dict[str, Any]) -> dict[str, Any]:
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    purge_document_graph(duck_path, document_id)
    return {"ok": True}
