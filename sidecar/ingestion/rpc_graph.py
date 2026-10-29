"""JSON-RPC: graph extraction and DuckDB purge."""

from __future__ import annotations

import asyncio
from typing import Any

from extraction.extraction_service import extract_chunk_graph
from extraction.graph_store import purge_document_graph


def rpc_extract_graph_chunk(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlitePath"])
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    chunk_id = str(params["chunkId"])
    chat_model = str(params.get("chatModel", "phi4-mini"))
    return asyncio.run(
        extract_chunk_graph(sqlite_path, duck_path, document_id, chunk_id, chat_model)
    )


def rpc_purge_graph_document(params: dict[str, Any]) -> dict[str, Any]:
    duck_path = str(params["duckPath"])
    document_id = str(params["documentId"])
    purge_document_graph(duck_path, document_id)
    return {"ok": True}
