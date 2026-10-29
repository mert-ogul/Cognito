"""Deterministic hybrid retrieval: vector top-k → graph ≤2 hops → grounded context."""

from __future__ import annotations

import re
import sqlite3
from typing import Any

import duckdb

from common.duckdb_util import ensure_schema
import sqlite_vec

from common.sqlite_vec_util import (
    MOCK_EMBED_DIM,
    connect_with_vec,
    mock_embed_enabled,
    table_dimension,
)
from common.ollama import embed_text
from extraction.node_normalizer import normalize_name

_CITE_MARK_RE = re.compile(r"\[\d+\]")
_HISTORY_TURN_CAP = 2000
_HISTORY_TOTAL_CAP = 4000
_GRAPH_BLOCK_CAP = 3000
_PROMPT_TOTAL_CAP = 24000


class EmbeddingDimMismatch(Exception):
    """Query embedding length does not match the stored index."""


def _indexed_chunk_count(sqlite_path: str) -> int:
    """Best-effort count of chunk rows (for query-time embed failure messaging)."""
    try:
        con = sqlite3.connect(sqlite_path)
        try:
            row = con.execute("SELECT COUNT(*) FROM chunks").fetchone()
            return int(row[0]) if row else 0
        finally:
            con.close()
    except Exception:
        return 0


def _query_embedding_failed(query: str, system: str) -> dict[str, Any]:
    return {
        "retrievalMode": "vector_only",
        "citations": [],
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": query},
        ],
        "graphAugmented": False,
        "graphNodeIds": [],
        "queryEmbeddingFailed": True,
    }


def vector_top_k(sqlite_path: str, embed_model: str, query: str, k: int) -> list[dict[str, Any]]:
    con = connect_with_vec(sqlite_path)
    dim = table_dimension(con)
    if dim is None:
        con.close()
        return []

    try:
        qv = list(
            embed_text(embed_model, query) if not mock_embed_enabled() else [0.0] * MOCK_EMBED_DIM
        )
    except Exception:
        con.close()
        return []
    if len(qv) != dim:
        con.close()
        raise EmbeddingDimMismatch(f"query embedding has {len(qv)} values; index uses {dim}")
    qblob = sqlite_vec.serialize_float32(list(qv))
    rows = con.execute(
        """
        SELECT chunk_id, distance
        FROM chunk_embeddings
        WHERE embedding MATCH ?
        AND k = ?
        """,
        (qblob, k),
    ).fetchall()
    out: list[dict[str, Any]] = []
    for chunk_id, _dist in rows:
        r = con.execute(
            """
            SELECT c.document_id, c.text, c.page_start, c.page_end, d.title, d.path
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.id = ? AND d.status = 'indexed'
            """,
            (chunk_id,),
        ).fetchone()
        if not r:
            continue
        did, text, ps, pe, title, dpath = r
        out.append(
            {
                "chunk_id": chunk_id,
                "document_id": did,
                "text": text,
                "page_start": ps,
                "page_end": pe,
                "title": title,
                "path": dpath,
            }
        )
    con.close()
    return out


def _undirected_neighbors(con: duckdb.DuckDBPyConnection, nid: str) -> set[str]:
    rows = con.execute(
        """
        SELECT CASE WHEN source_node_id = ? THEN target_node_id ELSE source_node_id END
        FROM graph_edges WHERE source_node_id = ? OR target_node_id = ?
        """,
        [nid, nid, nid],
    ).fetchall()
    return {str(r[0]) for r in rows}


def _two_hop_subgraph(
    con: duckdb.DuckDBPyConnection, seed_ids: list[str]
) -> tuple[list[tuple], list[tuple], list[str]]:
    """Returns (node_rows, edge_rows, ordered_node_ids_for_context)."""
    if not seed_ids:
        return [], [], []
    hop1: set[str] = set()
    for sid in seed_ids[:12]:
        hop1 |= _undirected_neighbors(con, sid)
    hop1 -= set(seed_ids)
    hop2: set[str] = set()
    for n in list(hop1)[:24]:
        hop2 |= _undirected_neighbors(con, n)
    all_ids = set(seed_ids) | hop1 | hop2
    all_ids = {x for x in all_ids if x}

    if not all_ids:
        return [], [], []

    ph = ",".join(["?"] * len(all_ids))
    nrows = con.execute(
        f"SELECT node_id, name, kind FROM graph_nodes WHERE node_id IN ({ph})",
        list(all_ids),
    ).fetchall()

    ids = list(all_ids)
    ph2 = ",".join(["?"] * len(ids))
    erows = con.execute(
        f"""
        SELECT edge_id, source_node_id, target_node_id, relation, document_id, chunk_id
        FROM graph_edges
        WHERE source_node_id IN ({ph2}) AND target_node_id IN ({ph2})
        """,
        ids + ids,
    ).fetchall()

    ordered = [str(r[0]) for r in nrows][:48]
    return nrows, erows, ordered


def _seeds_from_chunk_edges(
    con: duckdb.DuckDBPyConnection, chunk_ids: list[str]
) -> list[str]:
    if not chunk_ids:
        return []
    ph = ",".join(["?"] * len(chunk_ids))
    rows = con.execute(
        f"""
        SELECT DISTINCT source_node_id, target_node_id
        FROM graph_edges
        WHERE chunk_id IN ({ph})
        """,
        chunk_ids,
    ).fetchall()
    out: list[str] = []
    seen: set[str] = set()
    for s, t in rows:
        for nid in (str(s), str(t)):
            if nid not in seen:
                seen.add(nid)
                out.append(nid)
    return out


def _seeds_from_question_names(
    con: duckdb.DuckDBPyConnection, question: str
) -> list[str]:
    nq = normalize_name(question)
    if not nq:
        return []
    rows = con.execute(
        "SELECT node_id, normalized_name FROM graph_nodes WHERE length(normalized_name) >= 3"
    ).fetchall()
    out: list[str] = []
    for nid, nn in rows:
        name = str(nn)
        if re.search(rf"(?<!\w){re.escape(name)}(?!\w)", nq):
            out.append(str(nid))
    return out


def _node_ids_for_chunk(con: duckdb.DuckDBPyConnection, chunk_id: str) -> list[str]:
    rows = con.execute(
        """
        SELECT DISTINCT source_node_id, target_node_id
        FROM graph_edges WHERE chunk_id = ?
        """,
        [chunk_id],
    ).fetchall()
    out: list[str] = []
    seen: set[str] = set()
    for s, t in rows:
        for nid in (str(s), str(t)):
            if nid not in seen:
                seen.add(nid)
                out.append(nid)
    return out


def _strip_cite_markers(text: str) -> str:
    return _CITE_MARK_RE.sub("", text)


def prepare_history(history: list[dict[str, Any]] | None) -> list[dict[str, str]]:
    prepared: list[dict[str, str]] = []
    for turn in history or []:
        role = str(turn.get("role") or "")
        if role not in ("user", "assistant", "system"):
            continue
        content = str(turn.get("content") or "")
        if role == "assistant":
            content = _strip_cite_markers(content)
        content = content[:_HISTORY_TURN_CAP]
        if not content.strip():
            continue
        prepared.append({"role": role, "content": content})
    while sum(len(t["content"]) for t in prepared) > _HISTORY_TOTAL_CAP and prepared:
        prepared.pop(0)
    return prepared


def embed_query_text(query: str, history: list[dict[str, Any]] | None) -> str:
    prev_user = None
    for turn in reversed(history or []):
        if str(turn.get("role") or "") == "user":
            prev_user = str(turn.get("content") or "").strip()
            break
    if prev_user:
        return f"{prev_user}\n{query}"
    return query


def prepare_chat_retrieval(
    sqlite_path: str,
    duck_path: str | None,
    query: str,
    embed_model: str,
    top_k: int = 12,
    max_chunks_in_prompt: int = 8,
    max_evidence_chars: int = 9000,
    history: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """
    Deterministic pipeline (no routing classifier):
    1) vector top-k chunks
    2) graph seeds from chunk edges + question entity names
    3) up to 2-hop subgraph when DuckDB available
    4) grounded prompt messages for the chat model
    """
    hist_raw = list(history or [])
    search_text = embed_query_text(query, hist_raw)
    try:
        chunks = vector_top_k(sqlite_path, embed_model, search_text, top_k)
    except EmbeddingDimMismatch:
        return _query_embedding_failed(
            query,
            "The embedding model does not match the index. Open Settings and rebuild the index.",
        )
    hist_msgs = prepare_history(hist_raw)
    if not chunks:
        if _indexed_chunk_count(sqlite_path) > 0:
            return _query_embedding_failed(
                query,
                (
                    "Indexed content exists, but query embedding failed. Ensure Ollama is running on "
                    "127.0.0.1 and the embedding model in settings is installed (`ollama pull <model>`). "
                    "Vector retrieval cannot run until embeddings work. Do not invent sources."
                ),
            )
        return {
            "retrievalMode": "vector_only",
            "citations": [],
            "messages": [
                {
                    "role": "system",
                    "content": "You are a helpful assistant. There is no indexed content in the library yet.",
                },
                *hist_msgs,
                {"role": "user", "content": query},
            ],
            "graphAugmented": False,
            "graphNodeIds": [],
            "embedQuery": search_text,
        }

    prompt_chunks = chunks[:max_chunks_in_prompt]
    chunk_index = {c["chunk_id"]: i + 1 for i, c in enumerate(prompt_chunks)}
    retrieved_ids = [c["chunk_id"] for c in prompt_chunks]

    graph_block = ""
    graph_augmented = False
    graph_node_ids: list[str] = []
    edge_note_lines: list[str] = []
    name_by_id: dict[str, str] = {}
    chunk_node_ids: dict[str, list[str]] = {cid: [] for cid in retrieved_ids}

    if duck_path:
        try:
            ensure_schema(duck_path)
            gcon = duckdb.connect(duck_path)
            try:
                seeds: list[str] = []
                seen: set[str] = set()
                for nid in _seeds_from_chunk_edges(gcon, retrieved_ids) + _seeds_from_question_names(
                    gcon, query
                ):
                    if nid not in seen:
                        seen.add(nid)
                        seeds.append(nid)
                    if len(seeds) >= 12:
                        break
                for cid in retrieved_ids:
                    chunk_node_ids[cid] = _node_ids_for_chunk(gcon, cid)
                if seeds:
                    nrows, erows, ordered = _two_hop_subgraph(gcon, seeds)
                    name_by_id = {str(r[0]): str(r[1]) for r in nrows}
                    graph_node_ids = ordered[:32]
                    node_lines = [f"- {r[1]} ({r[2]})" for r in nrows[:20]]
                    for e in erows[:30]:
                        _eid, s, t, rel, _did, cid = e
                        src = name_by_id.get(str(s), str(s))
                        tgt = name_by_id.get(str(t), str(t))
                        k = chunk_index.get(str(cid))
                        if k is not None:
                            edge_note_lines.append(f"- {src} —[{rel}]→ {tgt} (from [{k}])")
                        else:
                            edge_note_lines.append(f"- {src} —[{rel}]→ {tgt}")
                    if nrows or erows:
                        graph_augmented = True
                        parts = ["GRAPH_EVIDENCE (relations supported by stored extractions):"]
                        if node_lines:
                            parts.append("Nodes:\n" + "\n".join(node_lines))
                        if edge_note_lines:
                            parts.append("Edges:\n" + "\n".join(edge_note_lines))
                        graph_block = "\n".join(parts)
            finally:
                gcon.close()
        except Exception:
            graph_augmented = False
            graph_block = ""

    if not graph_augmented or not graph_block.strip():
        retrieval_mode = "vector_only"
        graph_block = (
            "(Graph evidence is not available for this answer — using retrieved chunks only.)"
        )
        graph_augmented = False
    else:
        retrieval_mode = "hybrid"

    evidence_parts: list[str] = []
    citations: list[dict[str, Any]] = []
    for i, c in enumerate(prompt_chunks):
        n = i + 1
        snippet = (c["text"] or "")[:1200]
        title = c["title"] or ""
        page = c.get("page_start")
        if page is not None:
            label = f"[{n}] {title}, p. {page}"
        else:
            label = f"[{n}] {title}"
        evidence_parts.append(f"{label}\n{snippet}")
        nids = chunk_node_ids.get(c["chunk_id"], []) if retrieval_mode == "hybrid" else []
        citations.append(
            {
                "documentId": c["document_id"],
                "title": c["title"],
                "path": c["path"],
                "pageStart": c["page_start"],
                "pageEnd": c["page_end"],
                "chunkId": c["chunk_id"],
                "snippet": (c["text"] or "")[:400],
                "nodeIds": nids[:16],
                "index": n,
            }
        )

    evidence_body = "\n\n".join(evidence_parts)
    if len(evidence_body) > max_evidence_chars:
        evidence_body = evidence_body[:max_evidence_chars] + "\n…"

    if len(graph_block) > _GRAPH_BLOCK_CAP:
        graph_block = graph_block[:_GRAPH_BLOCK_CAP] + "\n…"

    sys_prompt = (
        "Answer only from the numbered passages. "
        "Put the passage number in square brackets right after the sentence it supports, like [2]. "
        'Never write ids, file paths, or the word "document" before a number. '
        "If the passages do not answer the question, say so in one sentence."
    )

    def _prompt_len(hist: list[dict[str, str]], graph: str) -> int:
        user = f"Question:\n{query}\n\n{evidence_body}\n\n{graph}"
        return len(sys_prompt) + sum(len(m["content"]) for m in hist) + len(user)

    while _prompt_len(hist_msgs, graph_block) > _PROMPT_TOTAL_CAP and hist_msgs:
        hist_msgs.pop(0)
    if _prompt_len(hist_msgs, graph_block) > _PROMPT_TOTAL_CAP and graph_block:
        over = _prompt_len(hist_msgs, graph_block) - _PROMPT_TOTAL_CAP
        keep = max(0, len(graph_block) - over - 1)
        graph_block = (graph_block[:keep] + "…") if keep else ""
    user_content = f"Question:\n{query}\n\n{evidence_body}\n\n{graph_block}"

    return {
        "retrievalMode": retrieval_mode,
        "citations": citations,
        "messages": [
            {"role": "system", "content": sys_prompt},
            *hist_msgs,
            {"role": "user", "content": user_content},
        ],
        "graphAugmented": graph_augmented,
        "graphNodeIds": graph_node_ids,
        "embedQuery": search_text,
    }
