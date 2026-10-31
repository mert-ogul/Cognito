#!/usr/bin/env python3
"""JSON-RPC sidecar (stdin/stdout, one JSON object per line)."""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import time
from pathlib import Path
from typing import Any

_ROOT = Path(__file__).resolve().parent
if str(_ROOT) not in sys.path:
    sys.path.insert(0, str(_ROOT))

# Protocol replies stay on the real stdout; stray prints go to stderr (sidecar.log).
_PROTOCOL_OUT = sys.stdout
sys.stdout = sys.stderr

logging.basicConfig(
    stream=sys.stderr,
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
)
log = logging.getLogger("cognito.sidecar")

from common.model_store import download_models, hf_home, unload_gliner, warm_checking_models
from common.ollama import warm_model
from ingestion.rpc_entities import (
    rpc_extract_entities,
    rpc_purge_document_entities,
    rpc_recompute_links,
)
from ingestion.page_render import rpc_render_page, rpc_render_region
from ingestion.rpc_ingest import (
    rpc_chunk_documents,
    rpc_embed_chunks,
    rpc_parse_documents,
    rpc_purge_chunk_embeddings,
    rpc_reset_index,
)
from query.chat_pipeline import (
    prepare_chat_retrieval,
    prepare_related_retrieval,
    rerank_chat_candidates,
)
from query.check_answer import check_answer


def _reply(req_id: Any, result: Any) -> None:
    msg = {"jsonrpc": "2.0", "id": req_id, "result": result}
    _PROTOCOL_OUT.write(json.dumps(msg) + "\n")
    _PROTOCOL_OUT.flush()


def _err(req_id: Any, code: int, message: str) -> None:
    msg = {"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}}
    _PROTOCOL_OUT.write(json.dumps(msg) + "\n")
    _PROTOCOL_OUT.flush()


def _progress_line(payload: dict[str, Any]) -> None:
    _PROTOCOL_OUT.write(json.dumps(payload) + "\n")
    _PROTOCOL_OUT.flush()


def run_download_models(app_data: str) -> int:
    os.environ["HF_HOME"] = str(hf_home(app_data))
    try:
        download_models(app_data, progress_cb=_progress_line)
    except Exception as exc:
        log.exception("download-models failed")
        _progress_line({"error": str(exc)})
        return 1
    return 0


def dispatch(method: str, params: dict) -> Any:
    if method == "ping":
        return {"pong": True}
    if method == "warm_model":
        chat = params.get("chat_model", "qwen3.5:9b")
        warm_model(chat)
        return {"warmed": True}
    if method == "warm_checking_models":
        return warm_checking_models(str(params["app_data"]))
    if method == "unload_entity_model":
        unload_gliner()
        return {"ok": True}
    if method == "check_answer":
        draft = params.get("draft")
        if draft is None:
            draft = params.get("answer")
        if not isinstance(draft, dict):
            draft = {}
        return check_answer(
            str(params["app_data"]),
            list(params.get("passages") or []),
            draft,
        )
    if method == "parse_documents":
        return rpc_parse_documents(params)
    if method == "chunk_documents":
        return rpc_chunk_documents(params)
    if method == "embed_chunks":
        return rpc_embed_chunks(params)
    if method == "purge_chunk_embeddings":
        return rpc_purge_chunk_embeddings(params)
    if method == "reset_index":
        return rpc_reset_index(params)
    if method == "extract_entities":
        return rpc_extract_entities(params)
    if method == "purge_document_entities":
        return rpc_purge_document_entities(params)
    if method == "recompute_links":
        return rpc_recompute_links(params)
    if method == "render_region":
        return rpc_render_region(params)
    if method == "render_page":
        return rpc_render_page(params)
    if method == "prepare_chat_retrieval":
        hist = params.get("history", [])
        if not isinstance(hist, list):
            hist = []
        scope = params.get("scope_ids")
        if not isinstance(scope, list):
            scope = None
        return prepare_chat_retrieval(
            str(params["sqlite_path"]),
            str(params["query"]),
            str(params.get("embed_model", "qwen3-embedding:0.6b")),
            int(params.get("top_k", 30)),
            history=hist,
            scope_ids=scope,
            app_data=str(params["app_data"]) if params.get("app_data") is not None else None,
            skip_rerank=bool(params.get("skip_rerank") or params.get("skipRerank")),
        )
    if method == "rerank_chat_candidates":
        cands = params.get("candidates") or []
        if not isinstance(cands, list):
            cands = []
        profile_matches = params.get("profile_matches") or params.get("profileMatches") or []
        if not isinstance(profile_matches, list):
            profile_matches = []
        labels = params.get("labels") or []
        if not isinstance(labels, list):
            labels = []
        scope_ids = params.get("scope_ids") or params.get("scopeIds") or []
        if not isinstance(scope_ids, list):
            scope_ids = []
        counts = params.get("candidate_counts") or params.get("candidateCounts") or {}
        if not isinstance(counts, dict):
            counts = {}
        return rerank_chat_candidates(
            str(params["app_data"]),
            str(params.get("query") or ""),
            cands,
            sqlite_path=str(params["sqlite_path"])
            if params.get("sqlite_path") is not None
            else None,
            embed_model=str(params["embed_model"])
            if params.get("embed_model") is not None
            else None,
            profile_matches=profile_matches,
            explicit_scope=bool(
                params.get("explicit_scope") or params.get("explicitScope")
            ),
            labels=[str(x) for x in labels],
            normalised_question=str(params["normalised_question"])
            if params.get("normalised_question") is not None
            else (
                str(params["normalisedQuestion"])
                if params.get("normalisedQuestion") is not None
                else None
            ),
            scope_ids=[str(x) for x in scope_ids],
            scope_reason=str(params.get("scope_reason") or params.get("scopeReason") or "")
            or None,
            candidate_counts={str(k): int(v) for k, v in counts.items()},
        )
    if method == "prepare_related_retrieval":
        return prepare_related_retrieval(
            str(params["sqlite_path"]),
            str(params.get("entity_id_a") or params.get("entityIdA") or ""),
            str(params.get("entity_id_b") or params.get("entityIdB") or ""),
            str(params.get("embed_model", "qwen3-embedding:0.6b")),
            app_data=str(params["app_data"]) if params.get("app_data") is not None else None,
            top_k=int(params.get("top_k", 5)),
        )
    raise ValueError(f"unknown method: {method}")


def main_rpc() -> None:
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
        started = time.perf_counter()
        try:
            result = dispatch(method, params if isinstance(params, dict) else {})
            elapsed_ms = (time.perf_counter() - started) * 1000.0
            log.info("rpc %s completed in %.1f ms", method, elapsed_ms)
            _reply(req_id, result)
        except Exception as exc:  # noqa: BLE001
            elapsed_ms = (time.perf_counter() - started) * 1000.0
            log.exception("rpc %s failed after %.1f ms", method, elapsed_ms)
            _err(req_id, -32000, str(exc))


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "download-models":
        parser = argparse.ArgumentParser(prog="main.py download-models")
        parser.add_argument("--app-data", required=True)
        args = parser.parse_args(sys.argv[2:])
        raise SystemExit(run_download_models(args.app_data))
    main_rpc()


if __name__ == "__main__":
    main()
