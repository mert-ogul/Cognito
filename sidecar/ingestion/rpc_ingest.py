"""JSON-RPC handlers: parse_documents, chunk_documents, embed_chunks, purge_chunk_embeddings."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any


from common.ollama import embed_prefixes, embed_texts
from common.sqlite_vec_util import (
    MOCK_EMBED_DIM,
    connect_with_vec,
    delete_embeddings_for_chunk_ids,
    ensure_chunk_embedding_table,
    insert_chunk_embedding,
    mock_embed_enabled,
    table_dimension,
)
from ingestion.chunking import chunk_semantic, estimate_tokens, extract_chunk_labels, hash_chunk_text
from ingestion.parsing import parse_file

_EMBED_BATCH = 32


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
            trail = " > ".join(t for t in ch.heading_trail if t)
            role = ch.role if ch.role in ("body", "references") else "body"
            labels = extract_chunk_labels(ch.text)
            regions_json = (
                json.dumps(ch.regions) if ch.regions else None
            )
            chunks.append(
                {
                    "ordinal": ord_i,
                    "text": ch.text,
                    "tokenCount": estimate_tokens(ch.text),
                    "contentHash": hash_chunk_text(ch.text),
                    "pageStart": ch.page_start,
                    "pageEnd": ch.page_end,
                    "headingTrail": trail or None,
                    "role": role,
                    "labels": json.dumps(labels),
                    "regions": regions_json,
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


def _embed_document_text(
    model: str, title: str, heading_trail: str | None, text: str
) -> str:
    _, doc_prefix = embed_prefixes(model)
    parts = [doc_prefix + (title or "").strip()]
    trail = (heading_trail or "").strip()
    if trail:
        parts.append(trail)
    parts.append(text)
    return "\n".join(parts)


def rpc_embed_chunks(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlitePath"])
    model = str(params["embedModel"])
    items = params["items"]
    con = connect_with_vec(sqlite_path)
    failed: list[dict[str, str]] = []
    try:
        prepared: list[tuple[str, str]] = []
        for item in items:
            cid = str(item["chunkId"])
            text = str(item["text"])
            title = str(item.get("title") or "")
            trail = item.get("headingTrail")
            trail_s = str(trail) if trail else None
            embed_input = _embed_document_text(model, title, trail_s, text)[:8000]
            prepared.append((cid, embed_input))

        for start in range(0, len(prepared), _EMBED_BATCH):
            batch = prepared[start : start + _EMBED_BATCH]
            cids = [c for c, _ in batch]
            texts = [t for _, t in batch]
            try:
                if mock_embed_enabled():
                    vecs = [[0.0] * MOCK_EMBED_DIM for _ in texts]
                else:
                    vecs = embed_texts(model, texts)
                for cid, vec in zip(cids, vecs, strict=True):
                    try:
                        if table_dimension(con) is None:
                            ensure_chunk_embedding_table(con, len(vec))
                        insert_chunk_embedding(con, cid, vec)
                    except Exception as exc:  # noqa: BLE001
                        failed.append({"chunkId": cid, "error": str(exc)[:240]})
            except Exception as exc:  # noqa: BLE001
                err = str(exc)[:240]
                for cid in cids:
                    failed.append({"chunkId": cid, "error": err})
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
    con = connect_with_vec(sqlite_path)
    try:
        con.execute("DROP TABLE IF EXISTS chunk_embeddings")
        con.commit()
    finally:
        con.close()
    from ingestion.rpc_entities import clear_entity_buffers

    clear_entity_buffers()
    return {"ok": True}
