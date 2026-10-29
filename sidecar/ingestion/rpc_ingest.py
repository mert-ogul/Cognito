"""JSON-RPC handlers: parse_documents, chunk_documents, embed_chunks, purge_chunk_embeddings."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import duckdb

from common.duckdb_util import ensure_schema
from common.ollama import embed_text
from common.sqlite_vec_util import (
    MOCK_EMBED_DIM,
    connect_with_vec,
    delete_embeddings_for_chunk_ids,
    ensure_chunk_embedding_table,
    insert_chunk_embedding,
    mock_embed_enabled,
    table_dimension,
)
from ingestion.chunking import chunk_semantic, estimate_tokens, hash_chunk_text
from ingestion.parsing import parse_file


def rpc_parse_documents(params: dict[str, Any]) -> dict[str, Any]:
    root = Path(params["vaultRoot"])
    paths = params["paths"]
    items: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for p in paths:
        path = Path(p)
        if not path.is_absolute():
            path = (root / path).resolve()
        try:
            items.append(parse_file(path))
        except (OSError, ValueError) as exc:
            errors.append({"path": str(path), "error": str(exc)[:512]})
    return {"items": items, "errors": errors}


def rpc_chunk_documents(params: dict[str, Any]) -> dict[str, Any]:
    out: list[dict[str, Any]] = []
    for it in params["items"]:
        ftype = str(it.get("fileType", "markdown"))
        pages = it.get("pages")
        if pages is not None and not isinstance(pages, list):
            pages = None
        drafts = chunk_semantic(str(it.get("text", "")), ftype, pages=pages)
        chunks = []
        for ord_i, ch in enumerate(drafts):
            chunks.append(
                {
                    "ordinal": ord_i,
                    "text": ch.text,
                    "tokenCount": estimate_tokens(ch.text),
                    "contentHash": hash_chunk_text(ch.text),
                    "pageStart": ch.page_start,
                    "pageEnd": ch.page_end,
                }
            )
        out.append(
            {
                "path": it["path"],
                "contentHash": it["contentHash"],
                "chunks": chunks,
            }
        )
    return {"items": out}


def _embed_one(model: str, text: str) -> list[float]:
    if mock_embed_enabled():
        return [0.0] * MOCK_EMBED_DIM
    return embed_text(model, text[:8000])


def rpc_embed_chunks(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlitePath"])
    model = str(params["embedModel"])
    items = params["items"]
    con = connect_with_vec(sqlite_path)
    failed: list[dict[str, str]] = []
    try:
        for item in items:
            cid = str(item["chunkId"])
            text = str(item["text"])
            try:
                vec = _embed_one(model, text)
                if table_dimension(con) is None:
                    ensure_chunk_embedding_table(con, len(vec))
                insert_chunk_embedding(con, cid, vec)
            except Exception as exc:  # noqa: BLE001
                failed.append({"chunkId": cid, "error": str(exc)[:240]})
        con.commit()
    finally:
        con.close()
    return {"ok": True, "failed": failed}


def rpc_purge_chunk_embeddings(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlitePath"])
    ids = [str(x) for x in params.get("chunkIds", [])]
    if not ids:
        return {"ok": True}
    con = connect_with_vec(sqlite_path)
    try:
        if table_dimension(con) is None:
            return {"ok": True}
        delete_embeddings_for_chunk_ids(con, ids)
        con.commit()
    finally:
        con.close()
    return {"ok": True}


def rpc_reset_index(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlitePath"])
    duck_path = str(params["duckPath"])
    con = connect_with_vec(sqlite_path)
    try:
        con.execute("DROP TABLE IF EXISTS chunk_embeddings")
        con.commit()
    finally:
        con.close()
    ensure_schema(duck_path)
    duck = duckdb.connect(duck_path)
    try:
        duck.execute("DELETE FROM graph_edges")
        duck.execute("DELETE FROM graph_nodes")
    finally:
        duck.close()
    return {"ok": True}
