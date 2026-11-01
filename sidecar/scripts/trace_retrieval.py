#!/usr/bin/env python3
"""Trace where a known passage sits in Cognito retrieval.

Prints ranks for chunks whose text contains --find, across keyword, vector,
entity, fusion, rerank pool, and final passages.

From the repo root:
  cd sidecar && uv run python scripts/trace_retrieval.py \\
    --sqlite PATH --question "..." --find "It's a 1202"
"""

from __future__ import annotations

import argparse
import os
import sqlite3
import sys
from pathlib import Path
from typing import Any

_SIDECAR = Path(__file__).resolve().parents[1]
if str(_SIDECAR) not in sys.path:
    sys.path.insert(0, str(_SIDECAR))

_BUNDLE_ID = "com.mertogul.cognito"
_DEFAULT_EMBED = "qwen3-embedding:0.6b"


def _default_app_data() -> Path:
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / _BUNDLE_ID
    if sys.platform == "win32":
        base = os.environ.get("APPDATA") or str(Path.home() / "AppData" / "Roaming")
        return Path(base) / _BUNDLE_ID
    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg) / _BUNDLE_ID
    return Path.home() / ".local" / "share" / _BUNDLE_ID


def _default_sqlite() -> Path | None:
    env = os.environ.get("COGNITO_SQLITE")
    if env:
        return Path(env).expanduser()
    p = _default_app_data() / "vault.sqlite"
    return p if p.is_file() else None


def _read_setting(sqlite_path: Path, key: str) -> str | None:
    from common.sqlite_vec_util import require_sqlite_path

    try:
        path = require_sqlite_path(str(sqlite_path))
        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        try:
            row = con.execute("SELECT value FROM settings WHERE key = ?", (key,)).fetchone()
            return str(row[0]) if row and row[0] is not None else None
        finally:
            con.close()
    except (sqlite3.Error, ValueError):
        return None


def _rank_of(lst: list[dict[str, Any]], chunk_id: str) -> int | None:
    for i, item in enumerate(lst, start=1):
        cid = str(item.get("chunk_id") or item.get("chunkId") or "")
        if cid == chunk_id:
            return i
    return None


def _find_chunks(sqlite_path: Path, needle: str) -> list[dict[str, Any]]:
    con = sqlite3.connect(f"file:{sqlite_path}?mode=ro", uri=True)
    try:
        rows = con.execute(
            """
            SELECT c.id, c.document_id, c.text, c.role, c.page_start, c.page_end,
                   d.title, d.path, c.ordinal
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.text LIKE ?
              AND d.status = 'indexed'
            ORDER BY d.title, c.ordinal
            """,
            (f"%{needle}%",),
        ).fetchall()
    finally:
        con.close()
    out: list[dict[str, Any]] = []
    for cid, did, text, role, ps, pe, title, path, ordinal in rows:
        out.append(
            {
                "chunk_id": str(cid),
                "document_id": str(did),
                "text": str(text or ""),
                "role": str(role or "body"),
                "page_start": ps,
                "page_end": pe,
                "title": str(title or ""),
                "path": str(path or ""),
                "ordinal": ordinal,
            }
        )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sqlite", type=Path, default=None, help="Path to vault.sqlite")
    parser.add_argument("--question", required=True, help="User question")
    parser.add_argument("--find", required=True, help="Substring that identifies the target chunk")
    parser.add_argument("--embed-model", default=None)
    parser.add_argument("--app-data", type=Path, default=None)
    parser.add_argument(
        "--skip-rerank",
        action="store_true",
        help="Skip MiniLM rerank (fusion ranks only)",
    )
    args = parser.parse_args()

    raw_sqlite = args.sqlite or _default_sqlite()
    if raw_sqlite is None:
        raise SystemExit(
            "SQLite path is missing.\n"
            "Pass --sqlite PATH or set COGNITO_SQLITE "
            "(default app data vault.sqlite was not found)."
        )
    sqlite_path = raw_sqlite.expanduser().resolve()
    if not sqlite_path.is_file():
        raise SystemExit(
            f"SQLite not found: {sqlite_path}\nPass --sqlite PATH or set COGNITO_SQLITE."
        )

    app_data = (
        args.app_data.expanduser().resolve()
        if args.app_data
        else sqlite_path.parent
    )
    embed_model = (
        args.embed_model
        or _read_setting(sqlite_path, "embed_model")
        or _DEFAULT_EMBED
    )

    from common.text_norm import normalize_search_text
    from query.chat_pipeline import (
        _KEYWORD_K,
        _RERANK_POOL,
        _VECTOR_K,
        apply_label_boost,
        embed_query_text,
        exact_token_top_k,
        keyword_top_k,
        match_entity_mention_chunks,
        prepare_chat_retrieval,
        question_label_mentions,
        reciprocal_rank_fusion,
        rerank_chat_candidates,
        resolve_scope,
        vector_top_k,
        _chunks_by_ids,
        _merge_chunks_by_id,
        label_chunks_for_documents,
    )

    hits = _find_chunks(sqlite_path, args.find)
    if not hits:
        print(f"No indexed chunk contains {args.find!r}")
        return 1

    normalised_q = normalize_search_text(args.question)
    search_text = normalize_search_text(embed_query_text(normalised_q, []))
    scope_info = resolve_scope(str(sqlite_path), normalised_q, explicit_ids=None)
    profile_matches = list(scope_info.get("profileMatches") or [])
    effective_scope = list(scope_info.get("scopeIds") or [])
    scope_filter = effective_scope or None
    labels = question_label_mentions(normalised_q)

    print("=== query ===")
    print(f"question: {args.question}")
    print(f"normalised: {normalised_q}")
    print(f"search_text: {search_text}")
    print(f"embed_model: {embed_model}")
    print(f"scope_ids: {effective_scope}")
    print(f"scope_reason: {scope_info.get('scopeReason')}")
    print(f"scope_title: {scope_info.get('scopeTitle')}")
    print(f"labels: {labels}")
    print(f"profile_matches ({len(profile_matches)}):")
    for m in profile_matches[:8]:
        print(
            f"  score={m.get('score')} title={m.get('title')!r} "
            f"id={m.get('documentId')}"
        )
    print(f"find: {args.find!r} -> {len(hits)} chunk(s)")
    print()

    def _lists(doc_ids: list[str] | None) -> tuple[
        list[dict[str, Any]],
        list[dict[str, Any]],
        list[dict[str, Any]],
        list[dict[str, Any]],
        list[dict[str, Any]],
    ]:
        e_ids = match_entity_mention_chunks(str(sqlite_path), search_text)
        e_chunks = _chunks_by_ids(str(sqlite_path), e_ids)
        if doc_ids is not None:
            allow = set(doc_ids)
            e_chunks = [c for c in e_chunks if str(c.get("document_id")) in allow]
        try:
            v_chunks = vector_top_k(
                str(sqlite_path), embed_model, search_text, _VECTOR_K, document_ids=doc_ids
            )
        except Exception as exc:  # noqa: BLE001
            print(f"vector_top_k failed (scope={doc_ids}): {exc}")
            v_chunks = []
        k_chunks = keyword_top_k(
            str(sqlite_path), search_text, _KEYWORD_K, document_ids=doc_ids
        )
        x_chunks = exact_token_top_k(
            str(sqlite_path), search_text, _KEYWORD_K, document_ids=doc_ids
        )
        l_extra: list[dict[str, Any]] = []
        if labels:
            label_docs = (
                [str(x) for x in doc_ids]
                if doc_ids
                else [str(m["documentId"]) for m in profile_matches[:3]]
            )
            if label_docs:
                l_extra = label_chunks_for_documents(str(sqlite_path), label_docs, labels)
        ranked = [v_chunks, k_chunks]
        if x_chunks:
            ranked.append(x_chunks)
        if e_chunks:
            ranked.append(e_chunks)
        fused_local = reciprocal_rank_fusion(ranked)
        fused_local = _merge_chunks_by_id(fused_local, l_extra)
        boosted_local = apply_label_boost(fused_local, labels)
        return (
            v_chunks,
            k_chunks,
            x_chunks,
            e_chunks,
            boosted_local,
            boosted_local[:_RERANK_POOL],
        )

    (
        vector_chunks,
        keyword_chunks,
        exact_chunks,
        entity_chunks,
        boosted,
        pool,
    ) = _lists(scope_filter)
    # Always also compute library-wide ranks so a wrong scope is visible.
    v_all, k_all, x_all, e_all, boosted_all, pool_all = _lists(None)
    print(
        f"library_wide sizes: keyword={len(k_all)} vector={len(v_all)} "
        f"exact={len(x_all)} entity={len(e_all)} fusion={len(boosted_all)}"
    )
    print(
        f"scoped sizes:      keyword={len(keyword_chunks)} vector={len(vector_chunks)} "
        f"exact={len(exact_chunks)} entity={len(entity_chunks)} fusion={len(boosted)}"
    )
    print()

    prep = prepare_chat_retrieval(
        str(sqlite_path),
        args.question,
        embed_model,
        app_data=str(app_data),
        skip_rerank=True,
    )
    candidates = list(prep.get("candidates") or [])
    final_cites: list[dict[str, Any]] = []
    rerank_scores: dict[str, float] = {}
    if not args.skip_rerank and candidates:
        rr = rerank_chat_candidates(
            str(app_data),
            str(prep.get("searchText") or search_text),
            candidates,
            sqlite_path=str(sqlite_path),
            embed_model=embed_model,
            profile_matches=list(prep.get("profileMatches") or []),
            explicit_scope=False,
            labels=list(prep.get("labels") or []),
            normalised_question=str(prep.get("normalisedQuestion") or normalised_q),
            scope_ids=list(prep.get("scopeIds") or []),
            scope_reason=str(prep.get("scopeReason") or ""),
            candidate_counts=dict(prep.get("candidateCounts") or {}),
        )
        final_cites = list(rr.get("citations") or [])
        # Rebuild ranked scores for pool order from candidates + rerank via pool re-score path
        from query.chat_pipeline import _rerank_pool

        ranked, rr_meta = _rerank_pool(str(app_data), str(prep.get("searchText") or search_text), pool)
        for rank, (chunk, score) in enumerate(ranked, start=1):
            cid = str(chunk.get("chunk_id") or "")
            rerank_scores[cid] = float(score)
            chunk["_rerank_rank"] = rank
        print(
            f"rerank_ms={rr_meta.get('rerankMs')} scored={rr_meta.get('scored')}/{len(pool)} "
            f"bestScore={rr.get('bestScore')} notFound={rr.get('notFound')}"
        )
        print(f"retrieval_ms={prep.get('retrievalMs')} final_passages={len(final_cites)}")
        print()
    else:
        final_cites = list(prep.get("citations") or [])
        print(f"skip_rerank=1 retrieval_ms={prep.get('retrievalMs')}")
        print()

    final_ids = {
        str(c.get("chunkId") or c.get("chunk_id") or "") for c in final_cites
    }

    for hit in hits:
        cid = hit["chunk_id"]
        print("=== chunk ===")
        print(f"id: {cid}")
        print(f"title: {hit['title']}")
        print(f"path: {hit['path']}")
        print(f"role: {hit['role']}  pages: {hit['page_start']}-{hit['page_end']}  ordinal: {hit['ordinal']}")
        snippet = hit["text"].replace("\n", " ")
        if len(snippet) > 220:
            snippet = snippet[:220] + "…"
        print(f"snippet: {snippet}")
        print("--- scoped (pipeline) ---")
        print(f"keyword_rank: {_rank_of(keyword_chunks, cid)} / {len(keyword_chunks)}")
        print(f"vector_rank:  {_rank_of(vector_chunks, cid)} / {len(vector_chunks)}")
        print(f"exact_rank:   {_rank_of(exact_chunks, cid)} / {len(exact_chunks)}")
        print(f"entity_rank:  {_rank_of(entity_chunks, cid)} / {len(entity_chunks)}")
        print(f"fusion_rank:  {_rank_of(boosted, cid)} / {len(boosted)}")
        in_pool = _rank_of(pool, cid)
        print(f"rerank_pool:  {in_pool} / {_RERANK_POOL} (in_pool={in_pool is not None})")
        print("--- library-wide (no scope) ---")
        print(f"keyword_rank: {_rank_of(k_all, cid)} / {len(k_all)}")
        print(f"vector_rank:  {_rank_of(v_all, cid)} / {len(v_all)}")
        print(f"exact_rank:   {_rank_of(x_all, cid)} / {len(x_all)}")
        print(f"entity_rank:  {_rank_of(e_all, cid)} / {len(e_all)}")
        print(f"fusion_rank:  {_rank_of(boosted_all, cid)} / {len(boosted_all)}")
        in_pool_all = _rank_of(pool_all, cid)
        print(f"rerank_pool:  {in_pool_all} / {_RERANK_POOL} (in_pool={in_pool_all is not None})")
        if cid in rerank_scores:
            ordered = sorted(rerank_scores.items(), key=lambda kv: (-kv[1], kv[0]))
            rr_rank = next((i for i, (oid, _) in enumerate(ordered, start=1) if oid == cid), None)
            print(f"rerank_score: {rerank_scores[cid]:.4f}  rerank_rank: {rr_rank}")
        else:
            print("rerank_score: (not in scoped pool)")
        print(f"in_final_passages: {cid in final_ids}")
        print()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
