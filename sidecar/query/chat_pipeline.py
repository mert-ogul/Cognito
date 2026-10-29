"""Deterministic hybrid retrieval: vector top-k → entity terms → graph ≤2 hops → grounded context."""

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


class EmbeddingDimMismatch(Exception):
    """Query embedding length does not match the stored index."""


def _words(text: str) -> set[str]:
    return set(re.findall(r"\b[\w\-]{3,}\b", text.lower()))


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


def _match_seed_nodes(con: duckdb.DuckDBPyConnection, terms: set[str]) -> list[str]:
    if not terms:
        return []
    norms = []
    for t in terms:
        n = normalize_name(t)
        if len(n) >= 2:
            norms.append(n)
    norms = list(dict.fromkeys(norms))[:24]
    if not norms:
        return []
    ph = ",".join(["?"] * len(norms))
    rows = con.execute(
        f"SELECT node_id FROM graph_nodes WHERE normalized_name IN ({ph}) LIMIT 16",
        norms,
    ).fetchall()
    return [str(r[0]) for r in rows]


def prepare_chat_retrieval(
    sqlite_path: str,
    duck_path: str | None,
    query: str,
    embed_model: str,
    top_k: int = 12,
    max_chunks_in_prompt: int = 8,
    max_evidence_chars: int = 9000,
) -> dict[str, Any]:
    """
    Deterministic pipeline (no routing classifier):
    1) vector top-k chunks
    2) candidate terms from query + retrieved text
    3) graph seeds + up to 2-hop subgraph when DuckDB available
    4) grounded prompt messages for the chat model
    """
    try:
        chunks = vector_top_k(sqlite_path, embed_model, query, top_k)
    except EmbeddingDimMismatch:
        return _query_embedding_failed(
            query,
            "The embedding model does not match the index. Open Settings and rebuild the index.",
        )
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
                {"role": "user", "content": query},
            ],
            "graphAugmented": False,
            "graphNodeIds": [],
        }

    terms: set[str] = _words(query)
    for c in chunks[:max_chunks_in_prompt]:
        terms |= _words(c["text"][:2000])

    graph_block = ""
    graph_augmented = False
    graph_node_ids: list[str] = []
    edge_note_lines: list[str] = []

    if duck_path:
        try:
            ensure_schema(duck_path)
            gcon = duckdb.connect(duck_path)
            try:
                seeds = _match_seed_nodes(gcon, terms)
                if seeds:
                    nrows, erows, ordered = _two_hop_subgraph(gcon, seeds)
                    graph_augmented = bool(nrows or erows)
                    graph_node_ids = ordered[:32]
                    node_lines = [f"- {r[1]} ({r[2]}) [node:{r[0]}]" for r in nrows[:20]]
                    for s, t, rel, did, cid in [
                        (e[1], e[2], e[3], e[4], e[5]) for e in erows[:30]
                    ]:
                        edge_note_lines.append(
                            f"- {s} —[{rel}]→ {t} (doc:{did[:8]}… chunk:{cid[:8]}…)"
                        )
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
    else:
        retrieval_mode = "hybrid"

    evidence_parts: list[str] = []
    citations: list[dict[str, Any]] = []
    for i, c in enumerate(chunks[:max_chunks_in_prompt]):
        label = f"CHUNK_EVIDENCE[{i + 1}]"
        snippet = (c["text"] or "")[:1200]
        evidence_parts.append(
            f"{label} title={c['title']} document_id={c['document_id']} chunk_id={c['chunk_id']}\n{snippet}"
        )
        nids = list(graph_node_ids[:16]) if retrieval_mode == "hybrid" else []
        citations.append(
            {
                "documentId": c["document_id"],
                "title": c["title"],
                "path": c["path"],
                "pageStart": c["page_start"],
                "pageEnd": c["page_end"],
                "chunkId": c["chunk_id"],
                "snippet": (c["text"] or "")[:400],
                "nodeIds": nids,
            }
        )

    evidence_body = "\n\n".join(evidence_parts)
    if len(evidence_body) > max_evidence_chars:
        evidence_body = evidence_body[:max_evidence_chars] + "\n…"

    sys_prompt = (
        "You are a careful assistant. Answer using ONLY the labeled CHUNK_EVIDENCE and GRAPH_EVIDENCE above. "
        "If GRAPH_EVIDENCE says it is unavailable, do not invent graph facts. "
        "If evidence is insufficient, say you are uncertain. Do not fabricate citations. "
        "Keep answers concise unless the user asks for detail."
    )
    user_content = (
        f"Question:\n{query}\n\n"
        f"{evidence_body}\n\n"
        f"{graph_block}"
    )

    return {
        "retrievalMode": retrieval_mode,
        "citations": citations,
        "messages": [
            {"role": "system", "content": sys_prompt},
            {"role": "user", "content": user_content[:14000]},
        ],
        "graphAugmented": graph_augmented,
        "graphNodeIds": graph_node_ids,
    }


def rpc_search_chunks(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(params["sqlite_path"])
    embed_model = str(params.get("embed_model", "nomic-embed-text"))
    query = str(params["query"])
    k = int(params.get("k", 12))
    try:
        chunks = vector_top_k(sqlite_path, embed_model, query, k)
    except EmbeddingDimMismatch:
        chunks = []
    return {"chunks": chunks}
