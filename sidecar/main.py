#!/usr/bin/env python3
"""JSON-RPC sidecar (stdin/stdout, one JSON object per line)."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

_ROOT = Path(__file__).resolve().parent
if str(_ROOT) not in sys.path:
    sys.path.insert(0, str(_ROOT))

from common.ollama import warm_model
from ingestion.rpc_graph import rpc_extract_graph_document, rpc_purge_graph_document
from ingestion.rpc_ingest import (
    rpc_chunk_documents,
    rpc_embed_chunks,
    rpc_parse_documents,
    rpc_purge_chunk_embeddings,
)
from common.duckdb_util import ensure_schema
from query.graph_read import node_detail as graph_node_detail
from query.graph_read import search_nodes as graph_search_nodes
from query.graph_read import snapshot as graph_snapshot
from query.chat_pipeline import prepare_chat_retrieval, rpc_search_chunks
from query.hybrid import health as hybrid_health
from query.hybrid import query_hybrid


def _reply(req_id: Any, result: dict) -> None:
    msg = {"jsonrpc": "2.0", "id": req_id, "result": result}
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def _err(req_id: Any, code: int, message: str) -> None:
    msg = {"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}}
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def dispatch(method: str, params: dict) -> dict:
    if method == "ping":
        return {"pong": True}
    if method == "health":
        embed = params.get("embed_model", "nomic-embed-text")
        chat = params.get("chat_model", "phi4-mini")
        return hybrid_health(embed, chat)
    if method == "warm_model":
        chat = params.get("chat_model", "phi4-mini")
        warm_model(chat)
        return {"warmed": True}
    if method == "parse_documents":
        return rpc_parse_documents(params)
    if method == "chunk_documents":
        return rpc_chunk_documents(params)
    if method == "embed_chunks":
        return rpc_embed_chunks(params)
    if method == "purge_chunk_embeddings":
        return rpc_purge_chunk_embeddings(params)
    if method == "extract_graph_document":
        return rpc_extract_graph_document(params)
    if method == "purge_graph_document":
        return rpc_purge_graph_document(params)
    if method == "prepare_chat_retrieval":
        return prepare_chat_retrieval(
            str(params["sqlite_path"]),
            str(params.get("duckdb_path") or "") or None,
            str(params["query"]),
            str(params.get("embed_model", "nomic-embed-text")),
            int(params.get("top_k", 12)),
        )
    if method == "search_chunks":
        return rpc_search_chunks(params)
    if method == "query_hybrid":
        return query_hybrid(
            params["sqlite_path"],
            params["duckdb_path"],
            params["query"],
            params.get("chat_model", "phi4-mini"),
            params.get("embed_model", "nomic-embed-text"),
        )
    if method == "get_graph_snapshot":
        return graph_snapshot(
            params["duckdb_path"],
            int(params.get("limit_nodes", 400)),
        )
    if method == "search_graph":
        return graph_search_nodes(
            params["duckdb_path"],
            params["query"],
            int(params.get("limit", 50)),
        )
    if method == "get_graph_node_detail":
        return graph_node_detail(
            params["duckdb_path"],
            str(params["node_id"]),
        )
    if method == "init_graph_store":
        ensure_schema(str(params["duckdb_path"]))
        return {"ok": True}
    raise ValueError(f"unknown method: {method}")


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError as exc:
            _err(None, -32700, str(exc))
            continue
        req_id = req.get("id")
        method = req.get("method")
        params = req.get("params") or {}
        if not isinstance(method, str):
            _err(req_id, -32600, "invalid method")
            continue
        try:
            result = dispatch(method, params if isinstance(params, dict) else {})
            _reply(req_id, result)
        except Exception as exc:  # noqa: BLE001 — surface to host
            _err(req_id, -32000, str(exc))


if __name__ == "__main__":
    main()
