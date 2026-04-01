"""JSON-RPC: graph extraction and DuckDB purge."""

from __future__ import annotations

import asyncio
from typing import Any

from extraction.extraction_service import extract_document_graph_run
from extraction.graph_store import purge_document_graph


def rpc_extract_graph_document(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlitePath"])
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    chat_model = str(params.get("chatModel", "phi4-mini"))
    try:
        out = asyncio.run(
            extract_document_graph_run(sqlite_path, duck_path, document_id, chat_model)
        )
        return out
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": str(exc)[:2048], "chunksProcessed": 0}


def rpc_purge_graph_document(params: dict[str, Any]) -> dict[str, Any]:
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    purge_document_graph(duck_path, document_id)
    return {"ok": True}
