"""Entity extraction, purge, and document-link recompute RPCs."""

from __future__ import annotations

import hashlib
import json
import re
import sqlite3
import struct
from typing import Any

import numpy as np
from rapidfuzz import fuzz

from common.model_store import extract_spans_batch, mock_models_enabled
from common.sqlite_vec_util import (
    connect_sqlite,
    connect_with_vec,
    mock_embed_enabled,
    require_sqlite_path,
    table_dimension,
)
from common.text_norm import normalize_date, normalize_name
from query.entity_labels import labels_for_document

_WINDOW_WORDS = 200
_WINDOW_OVERLAP = 30
_GLINER_THRESHOLD = 0.5
_CHUNK_LIMIT = 40
_SIMILAR_COSINE = 0.75
_SIMILAR_TOP_K = 3
_TITLE_PARTIAL_RATIO = 90
_TITLE_MIN_WORDS = 4
# GLiNER's word pattern; windows must stay under its 384-token cap.
_GLINER_WORD_RE = re.compile(r"\w+(?:[-_]\w+)*|\S")
_GENERIC_TAIL = frozenset(
    {
        "dataset",
        "data",
        "set",
        "model",
        "network",
        "networks",
        "algorithm",
        "method",
        "system",
        "framework",
        "corpus",
        "benchmark",
        "paper",
        "approach",
        "architecture",
    }
)
_ABBR_RE = re.compile(
    r"(?P<full>(?:[A-Z][A-Za-z0-9]+)(?:[\s\-]+(?:[A-Z][A-Za-z0-9]+)){1,10})"
    r"\s*\((?P<abbr>[A-Z][A-Z0-9]{1,12})\)"
)
_WORD_RE = re.compile(r"[^\W_]+(?:'[^\W_]+)?")
_YEAR_IN_TEXT = re.compile(r"\b(1[0-9]{3}|20[0-9]{2}|2100)\b")

# Raw spans buffered across incremental extract_entities calls, keyed by document id.
_doc_span_buffers: dict[str, list[dict[str, Any]]] = {}


def clear_entity_buffers() -> None:
    _doc_span_buffers.clear()


def _param(params: dict[str, Any], *keys: str, default: Any = None) -> Any:
    for key in keys:
        if key in params and params[key] is not None:
            return params[key]
    return default


def _entity_id(label: str, normalized: str) -> str:
    digest = hashlib.sha256(f"{label}\0{normalized}".encode("utf-8")).hexdigest()[:24]
    return f"ent_{digest}"


def _word_count(text: str) -> int:
    return len(_WORD_RE.findall(text or ""))


def _windows(
    text: str,
    max_words: int = _WINDOW_WORDS,
    overlap: int = _WINDOW_OVERLAP,
) -> list[tuple[int, int, str]]:
    """Return (start, end, window_text) char spans with a word overlap between windows."""
    if not text:
        return []
    words = list(_WORD_RE.finditer(text))
    if not words:
        return [(0, len(text), text)]
    step = max(1, max_words - max(0, overlap))
    out: list[tuple[int, int, str]] = []
    i = 0
    while i < len(words):
        j = min(i + max_words, len(words))
        start = words[i].start()
        end = words[j - 1].end()
        if i == 0:
            start = 0
        if j >= len(words):
            end = len(text)
        out.append((start, end, text[start:end]))
        if j >= len(words):
            break
        i = i + step
    return out


def gliner_token_count(text: str) -> int:
    return len(_GLINER_WORD_RE.findall(text or ""))


def _abbr_pairs_from_text(text: str) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    for m in _ABBR_RE.finditer(text or ""):
        full = normalize_name(m.group("full"))
        abbr = normalize_name(m.group("abbr"))
        if full and abbr:
            pairs.append((full, abbr))
    return pairs


def merge_within_doc(
    spans: list[dict[str, Any]],
    context_text: str = "",
) -> list[dict[str, Any]]:
    """Merge substring+generic and Full Name (ABBR) variants within one document."""
    if not spans:
        return []

    abbr_pairs = _abbr_pairs_from_text(context_text)
    for sp in spans:
        abbr_pairs.extend(_abbr_pairs_from_text(str(sp.get("text") or "")))

    items: list[dict[str, Any]] = []
    for sp in spans:
        label = str(sp.get("label") or "").strip()
        text = str(sp.get("text") or "").strip()
        if not label or not text:
            continue
        if label.lower() == "date":
            nd = normalize_date(text)
            if not nd:
                ym = _YEAR_IN_TEXT.search(text)
                if not ym:
                    continue
                nd = ym.group(1)
            name = nd
            normalized = nd
        else:
            name = text
            normalized = normalize_name(text)
            if not normalized:
                continue
        items.append(
            {
                "label": label,
                "name": name,
                "normalized": normalized,
                "chunk_id": sp.get("chunk_id"),
                "document_id": sp.get("document_id"),
                "start": int(sp.get("start") or 0),
                "end": int(sp.get("end") or 0),
                "score": float(sp.get("score") or 0.0),
            }
        )

    parent: dict[tuple[str, str], tuple[str, str]] = {}
    display: dict[tuple[str, str], str] = {}
    label_disp: dict[str, str] = {}

    def find(key: tuple[str, str]) -> tuple[str, str]:
        while parent.get(key, key) != key:
            parent[key] = parent.get(parent[key], parent[key])
            key = parent[key]
        return key

    def union(a: tuple[str, str], b: tuple[str, str]) -> None:
        ra, rb = find(a), find(b)
        if ra == rb:
            return
        keep, drop = ra, rb
        if len(rb[1]) > len(ra[1]):
            keep, drop = rb, ra
        parent[drop] = keep
        da, db = display.get(ra, ra[1]), display.get(rb, rb[1])
        display[keep] = da if len(da) >= len(db) else db

    for it in items:
        lab_l = it["label"].lower()
        if lab_l not in label_disp:
            label_disp[lab_l] = it["label"]
        key = (lab_l, it["normalized"])
        parent.setdefault(key, key)
        prev = display.get(key, "")
        if len(it["name"]) > len(prev):
            display[key] = it["name"]
        elif key not in display:
            display[key] = it["name"]

    for full, abbr in abbr_pairs:
        for lab, norm in list(parent.keys()):
            if norm == full and (lab, abbr) in parent:
                union((lab, full), (lab, abbr))

    keys = list(parent.keys())
    for i, a in enumerate(keys):
        for b in keys[i + 1 :]:
            if a[0] != b[0]:
                continue
            na, nb = a[1], b[1]
            if na == nb:
                continue
            shorter, longer = (na, nb) if len(na) <= len(nb) else (nb, na)
            if not longer.startswith(shorter):
                continue
            rest = longer[len(shorter) :].strip()
            if not rest:
                union(a, b)
                continue
            parts = [p for p in rest.split() if p]
            if parts and all(p in _GENERIC_TAIL for p in parts):
                union(a, b)

    merged: list[dict[str, Any]] = []
    for it in items:
        root = find((it["label"].lower(), it["normalized"]))
        merged.append(
            {
                **it,
                "label": label_disp.get(root[0], it["label"]),
                "normalized": root[1],
                "name": display.get(root, it["name"]),
            }
        )
    return merged


def _cosine(a: np.ndarray, b: np.ndarray) -> float:
    na = float(np.linalg.norm(a))
    nb = float(np.linalg.norm(b))
    if na <= 0.0 or nb <= 0.0:
        return 0.0
    return float(np.dot(a, b) / (na * nb))


def _blob_to_vec(blob: bytes) -> np.ndarray:
    n = len(blob) // 4
    return np.asarray(struct.unpack(f"{n}f", blob), dtype=np.float64)


def similar_links(
    doc_ids: list[str],
    vectors: np.ndarray,
    *,
    top_k: int = _SIMILAR_TOP_K,
    min_cosine: float = _SIMILAR_COSINE,
) -> list[tuple[str, str, float]]:
    """Each doc → up to top_k nearest others with cosine >= min_cosine."""
    n = len(doc_ids)
    if n < 2 or vectors.shape[0] != n:
        return []
    out: list[tuple[str, str, float]] = []
    for i in range(n):
        scores: list[tuple[float, str]] = []
        for j in range(n):
            if i == j:
                continue
            scores.append((_cosine(vectors[i], vectors[j]), doc_ids[j]))
        scores.sort(key=lambda t: (-t[0], t[1]))
        for score, tid in scores[:top_k]:
            if score >= min_cosine:
                out.append((doc_ids[i], tid, score))
    return out


_FTS_STOPWORDS = frozenset(
    {
        "a",
        "an",
        "and",
        "are",
        "as",
        "at",
        "be",
        "by",
        "for",
        "from",
        "how",
        "in",
        "is",
        "it",
        "of",
        "on",
        "or",
        "that",
        "the",
        "this",
        "to",
        "was",
        "what",
        "when",
        "where",
        "which",
        "who",
        "with",
    }
)


def _significant_title_words(title: str) -> list[str]:
    out: list[str] = []
    for m in _WORD_RE.finditer(title or ""):
        t = m.group(0).lower()
        if t.endswith("'s"):
            t = t[:-2]
        if len(t) < 2 or t in _FTS_STOPWORDS:
            continue
        out.append(t)
    return out


def _fts_phrase_from_title(title: str) -> str | None:
    words = _significant_title_words(title)
    if not words:
        return None
    return " ".join('"' + w.replace('"', '""') + '"' for w in words)


def _has_chunks_fts(con: sqlite3.Connection) -> bool:
    row = con.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='chunks_fts'"
    ).fetchone()
    return row is not None


def _candidate_chunks_for_title(
    con: sqlite3.Connection | None,
    document_id: str,
    title: str,
    chunks: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """FTS5 phrase filter on title words; fall back to all chunks when FTS is unavailable."""
    if con is None or not _has_chunks_fts(con):
        return chunks
    phrase = _fts_phrase_from_title(title)
    if not phrase:
        return chunks
    try:
        rows = con.execute(
            """
            SELECT c.id, c.text
            FROM chunks_fts
            JOIN chunks c ON c.rowid = chunks_fts.rowid
            WHERE c.document_id = ?
              AND chunks_fts MATCH ?
            ORDER BY c.ordinal
            """,
            (document_id, phrase),
        ).fetchall()
    except sqlite3.OperationalError:
        return chunks
    if not rows:
        return []
    by_id = {str(c.get("id") or ""): c for c in chunks}
    out: list[dict[str, Any]] = []
    for cid, text in rows:
        key = str(cid)
        if key in by_id:
            out.append(by_id[key])
        else:
            out.append({"id": key, "text": str(text or "")})
    return out


def _phrase_in_text(haystack_norm: str, needle: str) -> bool:
    n = normalize_name(needle)
    if not n:
        return False
    # Whole phrase: pad with spaces on normalized whitespace form.
    h = f" {haystack_norm} "
    return f" {n} " in h


def find_mentions_links(
    docs: list[dict[str, Any]],
    con: sqlite3.Connection | None = None,
) -> list[tuple[str, str, str | None, float]]:
    """Return (source_id, target_id, evidence_chunk_id, weight) for title/alias mentions."""
    links: list[tuple[str, str, str | None, float]] = []
    for src in docs:
        sid = str(src["id"])
        chunks = list(src.get("chunks") or [])
        for tgt in docs:
            tid = str(tgt["id"])
            if sid == tid:
                continue
            title = str(tgt.get("title") or "").strip()
            aliases = [str(a) for a in (tgt.get("aliases") or []) if str(a).strip()]
            evidence: str | None = None
            hit = False
            title_words = _word_count(title)
            title_norm = normalize_name(title)
            if title_words >= _TITLE_MIN_WORDS and title_norm:
                for ch in _candidate_chunks_for_title(con, sid, title, chunks):
                    text = str(ch.get("text") or "")
                    text_norm = normalize_name(text)
                    cid = str(ch.get("id") or "") or None
                    if fuzz.partial_ratio(title_norm, text_norm) >= _TITLE_PARTIAL_RATIO:
                        hit = True
                        evidence = cid
                        break
            if not hit:
                for ch in chunks:
                    text = str(ch.get("text") or "")
                    text_norm = normalize_name(text)
                    cid = str(ch.get("id") or "") or None
                    for alias in aliases:
                        if _phrase_in_text(text_norm, alias):
                            hit = True
                            evidence = cid
                            break
                    if hit:
                        break
            if hit:
                links.append((sid, tid, evidence, 1.0))
    return links


def _refresh_entity_counts(con: sqlite3.Connection, entity_ids: list[str] | None = None) -> None:
    if entity_ids is not None and not entity_ids:
        return
    if entity_ids is None:
        rows = con.execute("SELECT id FROM entities").fetchall()
        ids = [str(r[0]) for r in rows]
    else:
        ids = list(dict.fromkeys(entity_ids))
    for eid in ids:
        ment = con.execute(
            "SELECT COUNT(*), COUNT(DISTINCT document_id) FROM mentions WHERE entity_id = ?",
            (eid,),
        ).fetchone()
        mention_count = int(ment[0] or 0)
        doc_count = int(ment[1] or 0)
        if mention_count == 0:
            con.execute("DELETE FROM entity_aliases WHERE entity_id = ?", (eid,))
            con.execute("DELETE FROM entities WHERE id = ?", (eid,))
        else:
            con.execute(
                "UPDATE entities SET mention_count = ?, doc_count = ? WHERE id = ?",
                (mention_count, doc_count, eid),
            )


def _upsert_alias(con: sqlite3.Connection, entity_id: str, alias: str) -> None:
    a = (alias or "").strip()
    if not a:
        return
    con.execute(
        """
        INSERT OR IGNORE INTO entity_aliases(entity_id, alias) VALUES (?, ?)
        """,
        (entity_id, a),
    )


def rpc_purge_document_entities(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = require_sqlite_path(_param(params, "sqlitePath", "sqlite_path"))
    document_id = str(_param(params, "documentId", "document_id"))
    con = connect_sqlite(sqlite_path)
    try:
        entity_ids = [
            str(r[0])
            for r in con.execute(
                "SELECT DISTINCT entity_id FROM mentions WHERE document_id = ?",
                (document_id,),
            ).fetchall()
        ]
        con.execute("DELETE FROM mentions WHERE document_id = ?", (document_id,))
        _refresh_entity_counts(con, entity_ids)
        con.execute(
            "DELETE FROM document_links WHERE source_id = ? OR target_id = ?",
            (document_id, document_id),
        )
        con.commit()
    finally:
        con.close()
    return {"ok": True}


def _commit_document_mentions(
    con: sqlite3.Connection,
    document_id: str,
    raw_spans: list[dict[str, Any]],
    context_text: str,
) -> tuple[int, int]:
    old_ids = [
        str(r[0])
        for r in con.execute(
            "SELECT DISTINCT entity_id FROM mentions WHERE document_id = ?",
            (document_id,),
        ).fetchall()
    ]
    con.execute("DELETE FROM mentions WHERE document_id = ?", (document_id,))
    _refresh_entity_counts(con, old_ids)

    merged = merge_within_doc(raw_spans, context_text=context_text)
    mention_rows = 0
    entity_ids: set[str] = set()
    for it in merged:
        label = str(it["label"])
        name = str(it["name"])
        normalized = str(it["normalized"])
        eid = _entity_id(label.lower(), normalized)
        entity_ids.add(eid)
        row = con.execute("SELECT id, name FROM entities WHERE id = ?", (eid,)).fetchone()
        if row:
            if len(name) > len(str(row[1] or "")):
                con.execute(
                    "UPDATE entities SET name = ?, label = ? WHERE id = ?",
                    (name, label, eid),
                )
        else:
            existing = con.execute(
                """
                SELECT id, name FROM entities
                WHERE lower(label) = lower(?) AND normalized = ?
                """,
                (label, normalized),
            ).fetchone()
            if existing:
                eid = str(existing[0])
                entity_ids.add(eid)
                if len(name) > len(str(existing[1] or "")):
                    con.execute(
                        "UPDATE entities SET name = ? WHERE id = ?",
                        (name, eid),
                    )
            else:
                con.execute(
                    """
                    INSERT INTO entities(
                      id, label, name, normalized, doc_count, mention_count
                    ) VALUES (?, ?, ?, ?, 0, 0)
                    """,
                    (eid, label, name, normalized),
                )
        surface = str(it.get("text") or name)
        if normalize_name(surface) and normalize_name(surface) != normalized:
            _upsert_alias(con, eid, surface)
        if name != surface:
            _upsert_alias(con, eid, name)

        con.execute(
            """
            INSERT INTO mentions(
              entity_id, chunk_id, document_id, start, end
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (
                eid,
                str(it["chunk_id"]),
                document_id,
                int(it["start"]),
                int(it["end"]),
            ),
        )
        mention_rows += 1

    _refresh_entity_counts(con, list(entity_ids))
    return mention_rows, len(entity_ids)


def rpc_extract_entities(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = require_sqlite_path(_param(params, "sqlitePath", "sqlite_path"))
    document_id = str(_param(params, "documentId", "document_id"))
    app_data = str(_param(params, "app_data", "appData") or "")
    extra_labels = [str(x).strip() for x in (_param(params, "labels") or []) if str(x).strip()]
    chunk_offset = int(_param(params, "chunkOffset", "chunk_offset", default=0) or 0)
    chunk_limit = int(
        _param(params, "chunkLimit", "chunk_limit", default=_CHUNK_LIMIT) or _CHUNK_LIMIT
    )
    if chunk_limit <= 0:
        chunk_limit = _CHUNK_LIMIT
    if chunk_offset < 0:
        chunk_offset = 0

    # Read with a short connection; run GLiNER with no DB handle open.
    con = connect_sqlite(sqlite_path)
    try:
        card_row = con.execute(
            "SELECT type FROM document_cards WHERE document_id = ?",
            (document_id,),
        ).fetchone()
        card_type = str(card_row[0]) if card_row and card_row[0] else None
        chunks = con.execute(
            """
            SELECT id, text, IFNULL(role, 'body')
            FROM chunks
            WHERE document_id = ?
            ORDER BY ordinal
            """,
            (document_id,),
        ).fetchall()
    finally:
        con.close()

    total_chunks = len(chunks)
    if card_type:
        labels = labels_for_document(card_type, extra_labels)
    else:
        has_refs = any(str(role or "body") == "references" for _cid, _t, role in chunks)
        labels = labels_for_document("paper" if has_refs else None, extra_labels)

    if chunk_offset == 0:
        _doc_span_buffers[document_id] = []

    buffer = _doc_span_buffers.setdefault(document_id, [])
    slice_chunks = chunks[chunk_offset : chunk_offset + chunk_limit]

    for chunk_id, text, role in slice_chunks:
        body = str(text or "")
        if str(role or "body") == "references":
            continue
        windows = _windows(body)
        if not windows:
            continue
        window_texts = [w[2] for w in windows]
        batched = extract_spans_batch(
            app_data, window_texts, labels, threshold=_GLINER_THRESHOLD
        )
        seen: set[tuple[str, int, int]] = set()
        for (start, _end, _window), spans in zip(windows, batched):
            for sp in spans:
                abs_start = int(sp["start"]) + start
                abs_end = int(sp["end"]) + start
                label = str(sp["label"] or "")
                key = (label, abs_start, abs_end)
                if key in seen:
                    continue
                seen.add(key)
                buffer.append(
                    {
                        "label": label,
                        "text": sp["text"],
                        "start": abs_start,
                        "end": abs_end,
                        "score": sp["score"],
                        "chunk_id": str(chunk_id),
                        "document_id": document_id,
                    }
                )

    next_offset = chunk_offset + len(slice_chunks)
    done = next_offset >= total_chunks
    if not done:
        return {
            "ok": True,
            "done": False,
            "nextOffset": next_offset,
            "totalChunks": total_chunks,
            "mentions": 0,
            "entities": 0,
        }

    context_text = "\n".join(str(t or "") for _cid, t, _role in chunks)
    con = connect_sqlite(sqlite_path)
    try:
        mention_rows, entity_count = _commit_document_mentions(
            con, document_id, list(buffer), context_text
        )
        con.commit()
    finally:
        con.close()
    _doc_span_buffers.pop(document_id, None)
    return {
        "ok": True,
        "done": True,
        "nextOffset": next_offset,
        "totalChunks": total_chunks,
        "mentions": mention_rows,
        "entities": entity_count,
    }


def _load_docs_for_links(con: sqlite3.Connection) -> list[dict[str, Any]]:
    docs: list[dict[str, Any]] = []
    has_cards = con.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='document_cards'"
    ).fetchone()
    if has_cards:
        rows = con.execute(
            """
            SELECT d.id, d.title, c.aliases, c.summary
            FROM documents d
            LEFT JOIN document_cards c ON c.document_id = d.id
            WHERE d.status = 'indexed'
            """
        ).fetchall()
    else:
        rows = con.execute(
            "SELECT id, title, NULL, NULL FROM documents WHERE status = 'indexed'"
        ).fetchall()
    for did, title, aliases_raw, summary_raw in rows:
        aliases: list[str] = []
        if aliases_raw:
            try:
                parsed = json.loads(aliases_raw)
                if isinstance(parsed, list):
                    aliases = [str(a) for a in parsed if str(a).strip()]
            except (TypeError, json.JSONDecodeError):
                aliases = []
        summary_bits: list[str] = []
        if summary_raw:
            try:
                parsed = json.loads(summary_raw)
                if isinstance(parsed, list):
                    for item in parsed:
                        if isinstance(item, dict):
                            s = str(item.get("sentence") or item.get("text") or "")
                        else:
                            s = str(item or "")
                        if s.strip():
                            summary_bits.append(s.strip())
            except (TypeError, json.JSONDecodeError):
                pass
        chunks = con.execute(
            """
            SELECT id, text FROM chunks
            WHERE document_id = ?
            ORDER BY ordinal
            """,
            (did,),
        ).fetchall()
        docs.append(
            {
                "id": str(did),
                "title": str(title or ""),
                "aliases": aliases,
                "summary": summary_bits,
                "chunks": [{"id": str(c[0]), "text": str(c[1] or "")} for c in chunks],
            }
        )
    return docs


def _mean_pool_doc_vectors(sqlite_path: str, doc_ids: list[str]) -> np.ndarray | None:
    con = connect_with_vec(sqlite_path)
    try:
        dim = table_dimension(con)
        if dim is None:
            return None
        vectors: list[np.ndarray] = []
        for did in doc_ids:
            rows = con.execute(
                """
                SELECT e.embedding
                FROM chunk_embeddings e
                JOIN chunks c ON c.id = e.chunk_id
                WHERE c.document_id = ?
                """,
                (did,),
            ).fetchall()
            if not rows:
                vectors.append(np.zeros(dim, dtype=np.float64))
                continue
            mats = [_blob_to_vec(r[0]) for r in rows if r[0] is not None]
            if not mats:
                vectors.append(np.zeros(dim, dtype=np.float64))
            else:
                vectors.append(np.mean(np.stack(mats, axis=0), axis=0))
        return np.stack(vectors, axis=0)
    finally:
        con.close()


def _mock_similar_links(docs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Deterministic fake similar links for tests."""
    ids = [str(d["id"]) for d in docs]
    links: list[dict[str, Any]] = []
    for i, sid in enumerate(ids):
        for j, tid in enumerate(ids):
            if i >= j:
                continue
            w = 0.8 - 0.01 * abs(i - j)
            if w >= _SIMILAR_COSINE:
                links.append(
                    {
                        "source_id": sid,
                        "target_id": tid,
                        "kind": "similar",
                        "weight": round(w, 4),
                        "evidence_chunk_id": None,
                    }
                )
                if (
                    len([x for x in links if x["source_id"] == sid and x["kind"] == "similar"])
                    >= _SIMILAR_TOP_K
                ):
                    break
    return links


def rpc_recompute_links(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = require_sqlite_path(_param(params, "sqlitePath", "sqlite_path"))

    con = connect_sqlite(sqlite_path)
    try:
        docs = _load_docs_for_links(con)
        mention_links = find_mentions_links(docs, con=con)
    finally:
        con.close()

    doc_ids = [str(d["id"]) for d in docs]
    similar_rows: list[tuple[str, str, float]] = []
    if mock_models_enabled() or mock_embed_enabled():
        similar_payload = _mock_similar_links(docs)
    else:
        similar_payload = []
        vecs = _mean_pool_doc_vectors(sqlite_path, doc_ids)
        if vecs is not None and len(doc_ids) >= 2:
            similar_rows = list(similar_links(doc_ids, vecs))

    con = connect_sqlite(sqlite_path)
    try:
        con.execute("DELETE FROM document_links")
        for sid, tid, evid, weight in mention_links:
            con.execute(
                """
                INSERT INTO document_links(
                  source_id, target_id, kind, weight, evidence_chunk_id
                ) VALUES (?, ?, 'mentions', ?, ?)
                """,
                (sid, tid, weight, evid),
            )
        similar_count = 0
        if mock_models_enabled() or mock_embed_enabled():
            for link in similar_payload:
                con.execute(
                    """
                    INSERT INTO document_links(
                      source_id, target_id, kind, weight, evidence_chunk_id
                    ) VALUES (?, ?, ?, ?, ?)
                    """,
                    (
                        link["source_id"],
                        link["target_id"],
                        link["kind"],
                        link["weight"],
                        link["evidence_chunk_id"],
                    ),
                )
                similar_count += 1
        else:
            for sid, tid, weight in similar_rows:
                con.execute(
                    """
                    INSERT INTO document_links(
                      source_id, target_id, kind, weight, evidence_chunk_id
                    ) VALUES (?, ?, 'similar', ?, NULL)
                    """,
                    (sid, tid, weight),
                )
                similar_count += 1
        con.commit()
        return {
            "ok": True,
            "mentionsLinks": len(mention_links),
            "similarLinks": similar_count,
        }
    finally:
        con.close()
