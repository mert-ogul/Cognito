"""Orchestrate LightRAG extraction and DuckDB writes for one document."""

from __future__ import annotations

import sqlite3

from extraction.extractor_config import ExtractorConfig
from extraction.graph_extractor import extract_chunks_sequential
from extraction.graph_store import (
    lightrag_maps_to_records,
    open_connection,
    purge_document_graph,
    upsert_entities_edges,
)


def _estimate_tokens(text: str) -> int:
    return max(1, len(text) // 4)


def load_document_chunks(sqlite_path: str, document_id: str) -> tuple[str, list[tuple[str, str, int, int]]]:
    """Returns (vault_path_or_title_hint, [(chunk_id, text, ordinal, tokens), ...])."""
    con = sqlite3.connect(sqlite_path)
    try:
        row = con.execute(
            "SELECT path FROM documents WHERE id = ?",
            [document_id],
        ).fetchone()
        path_hint = str(row[0]) if row else "document"
        cur = con.execute(
            """
            SELECT id, text, ordinal FROM chunks WHERE document_id = ?
            ORDER BY ordinal ASC
            """,
            [document_id],
        )
        out: list[tuple[str, str, int, int]] = []
        for cid, text, ord_i in cur.fetchall():
            t = str(text or "")
            out.append((str(cid), t, int(ord_i), _estimate_tokens(t)))
        return path_hint, out
    finally:
        con.close()


async def extract_document_graph_async(
    sqlite_path: str,
    duck_path: str,
    document_id: str,
    chat_model: str,
) -> dict:
    """
    Incremental graph extraction for one document. Purges prior graph rows for this document first.
    Returns { "ok": bool, "error"?: str, "chunksProcessed": int }
    """
    path_hint, chunks = load_document_chunks(sqlite_path, document_id)
    if not chunks:
        return {"ok": True, "chunksProcessed": 0}

    purge_document_graph(duck_path, document_id)
    cfg = ExtractorConfig(chat_model=chat_model)
    extracted = await extract_chunks_sequential(document_id, path_hint, chunks, cfg)
    con = open_connection(duck_path)
    try:
        for chunk_id, maybe_nodes, maybe_edges in extracted:
            ents, edges = lightrag_maps_to_records(maybe_nodes, maybe_edges)
            upsert_entities_edges(con, document_id, chunk_id, ents, edges)
        con.commit()
    finally:
        con.close()
    return {"ok": True, "chunksProcessed": len(chunks)}


async def extract_document_graph_run(
    sqlite_path: str,
    duck_path: str,
    document_id: str,
    chat_model: str,
) -> dict:
    return await extract_document_graph_async(sqlite_path, duck_path, document_id, chat_model)
