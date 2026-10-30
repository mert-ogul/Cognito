"""Entity extraction, purge, and document-link recompute RPCs."""

from __future__ import annotations

import hashlib
import json
import re
import sqlite3
import struct
import uuid
from typing import Any

import numpy as np
from rapidfuzz import fuzz

from common.model_store import extract_spans, mock_models_enabled, unload_gliner
from common.ollama import chat, embed_texts
from common.sqlite_vec_util import connect_with_vec, mock_embed_enabled, table_dimension
from common.text_norm import normalize_date, normalize_name
from query.entity_labels import labels_for_document

_WINDOW_WORDS = 350
_GLINER_THRESHOLD = 0.5
_EMBED_MERGE_COSINE = 0.92
_SIMILAR_COSINE = 0.75
_SIMILAR_TOP_K = 3
_TITLE_PARTIAL_RATIO = 90
_TITLE_MIN_WORDS = 4
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
_WORD_RE = re.compile(r"[0-9A-Za-z]+(?:'[0-9A-Za-z]+)?")


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


def _windows(text: str, max_words: int = _WINDOW_WORDS) -> list[tuple[int, int, str]]:
    """Return (start, end, window_text) char spans covering ~max_words each."""
    if not text:
        return []
    words = list(_WORD_RE.finditer(text))
    if not words:
        return [(0, len(text), text)]
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
        i = j
    return out


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
                continue
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


def pca_points(vectors: np.ndarray) -> np.ndarray:
    """Return Nx2 PCA coordinates via SVD. Stable for fixed inputs."""
    if vectors.size == 0:
        return np.zeros((0, 2), dtype=np.float64)
    n, d = vectors.shape
    if n == 1:
        return np.zeros((1, 2), dtype=np.float64)
    mean = vectors.mean(axis=0)
    centered = vectors - mean
    # Economy SVD on centered data.
    _u, _s, vt = np.linalg.svd(centered, full_matrices=False)
    comps = vt[:2].T  # d x 2
    if comps.shape[1] == 1:
        comps = np.concatenate([comps, np.zeros((d, 1))], axis=1)
    pts = centered @ comps
    if pts.shape[1] == 1:
        pts = np.concatenate([pts, np.zeros((n, 1))], axis=1)
    # Flip signs for stability: first nonzero in each component positive.
    for j in range(min(2, pts.shape[1])):
        col = pts[:, j]
        for v in col:
            if abs(v) > 1e-12:
                if v < 0:
                    pts[:, j] *= -1
                break
    return pts[:, :2]


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


def agglomerative_clusters(
    vectors: np.ndarray,
    *,
    target_size: int = 8,
) -> list[list[int]]:
    """Average-linkage clustering on cosine distance; cluster count from target size."""
    n = vectors.shape[0]
    if n == 0:
        return []
    if n == 1:
        return [[0]]
    # Cosine distance matrix.
    norms = np.linalg.norm(vectors, axis=1, keepdims=True)
    norms = np.maximum(norms, 1e-12)
    unit = vectors / norms
    sim = unit @ unit.T
    dist = np.clip(1.0 - sim, 0.0, 2.0)
    np.fill_diagonal(dist, 0.0)

    n_clusters = max(1, min(n, n // max(1, target_size)))
    if n <= 15:
        n_clusters = max(1, min(n, max(1, n // 3)))
    if n >= 20:
        # Aim for cluster sizes in ~3–15.
        n_clusters = max(2, min(n // 3, max(1, n // 8)))

    clusters: list[list[int]] = [[i] for i in range(n)]
    # Cluster distance = average linkage.
    while len(clusters) > n_clusters:
        best = None
        best_d = float("inf")
        for i in range(len(clusters)):
            for j in range(i + 1, len(clusters)):
                total = 0.0
                count = 0
                for a in clusters[i]:
                    for b in clusters[j]:
                        total += float(dist[a, b])
                        count += 1
                d = total / max(1, count)
                if d < best_d:
                    best_d = d
                    best = (i, j)
        if best is None:
            break
        i, j = best
        merged = clusters[i] + clusters[j]
        clusters = [c for k, c in enumerate(clusters) if k != i and k != j]
        clusters.append(merged)
    return clusters


def _phrase_in_text(haystack_norm: str, needle: str) -> bool:
    n = normalize_name(needle)
    if not n:
        return False
    # Whole phrase: pad with spaces on normalized whitespace form.
    h = f" {haystack_norm} "
    return f" {n} " in h


def find_mentions_links(
    docs: list[dict[str, Any]],
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
            for ch in chunks:
                text = str(ch.get("text") or "")
                text_norm = normalize_name(text)
                cid = str(ch.get("id") or "") or None
                if title_words >= _TITLE_MIN_WORDS and title_norm:
                    if fuzz.partial_ratio(title_norm, text_norm) >= _TITLE_PARTIAL_RATIO:
                        hit = True
                        evidence = cid
                        break
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


def _merge_entities(con: sqlite3.Connection, keep_id: str, drop_id: str) -> None:
    if keep_id == drop_id:
        return
    keep = con.execute(
        "SELECT label, name, normalized FROM entities WHERE id = ?", (keep_id,)
    ).fetchone()
    drop = con.execute(
        "SELECT label, name, normalized FROM entities WHERE id = ?", (drop_id,)
    ).fetchone()
    if not keep or not drop:
        return
    con.execute(
        "UPDATE mentions SET entity_id = ? WHERE entity_id = ?",
        (keep_id, drop_id),
    )
    # Collapse duplicate mention rows after merge.
    con.execute(
        """
        DELETE FROM mentions WHERE rowid NOT IN (
          SELECT MIN(rowid) FROM mentions
          GROUP BY entity_id, chunk_id, document_id, start, end
        )
        """
    )
    _upsert_alias(con, keep_id, str(drop[1]))
    _upsert_alias(con, keep_id, str(drop[2]))
    for (alias,) in con.execute(
        "SELECT alias FROM entity_aliases WHERE entity_id = ?", (drop_id,)
    ).fetchall():
        _upsert_alias(con, keep_id, str(alias))
    con.execute("DELETE FROM entity_aliases WHERE entity_id = ?", (drop_id,))
    con.execute("DELETE FROM entities WHERE id = ?", (drop_id,))
    _refresh_entity_counts(con, [keep_id])


def _cross_doc_embed_merge(con: sqlite3.Connection, embed_model: str | None) -> int:
    if mock_embed_enabled() or mock_models_enabled() or not embed_model:
        return 0
    rows = con.execute(
        "SELECT id, label, name, normalized, mention_count FROM entities"
    ).fetchall()
    by_label: dict[str, list[tuple[str, str, str, int]]] = {}
    for eid, label, name, _norm, mc in rows:
        by_label.setdefault(str(label).lower(), []).append(
            (str(eid), str(name), str(label), int(mc or 0))
        )
    merged = 0
    for _lab, group in by_label.items():
        if len(group) < 2:
            continue
        names = [g[1] for g in group]
        try:
            vecs = embed_texts(embed_model, names)
        except Exception:
            continue
        if len(vecs) != len(group):
            continue
        arr = [np.asarray(v, dtype=np.float64) for v in vecs]
        parent = list(range(len(group)))

        def find(i: int) -> int:
            while parent[i] != i:
                parent[i] = parent[parent[i]]
                i = parent[i]
            return i

        def union(i: int, j: int) -> None:
            ri, rj = find(i), find(j)
            if ri == rj:
                return
            # Keep more frequent mentions.
            if group[rj][3] > group[ri][3]:
                ri, rj = rj, ri
            elif group[rj][3] == group[ri][3] and group[rj][0] < group[ri][0]:
                ri, rj = rj, ri
            parent[rj] = ri

        for i in range(len(group)):
            for j in range(i + 1, len(group)):
                if _cosine(arr[i], arr[j]) >= _EMBED_MERGE_COSINE:
                    union(i, j)
        # Merge drops into roots.
        roots: dict[int, int] = {}
        for i in range(len(group)):
            r = find(i)
            roots.setdefault(r, r)
            if i != r:
                _merge_entities(con, group[r][0], group[i][0])
                merged += 1
    return merged


def rpc_purge_document_entities(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(_param(params, "sqlitePath", "sqlite_path"))
    document_id = str(_param(params, "documentId", "document_id"))
    con = sqlite3.connect(sqlite_path)
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
        con.execute("DELETE FROM document_points WHERE document_id = ?", (document_id,))
        # Drop topics that referenced this document (full recompute preferred; prune JSON).
        topic_rows = con.execute("SELECT id, document_ids FROM topics").fetchall()
        for tid, raw in topic_rows:
            try:
                ids = json.loads(raw or "[]")
            except (TypeError, json.JSONDecodeError):
                ids = []
            if not isinstance(ids, list):
                continue
            if document_id not in ids:
                continue
            left = [x for x in ids if str(x) != document_id]
            if not left:
                con.execute("DELETE FROM topics WHERE id = ?", (tid,))
            else:
                con.execute(
                    "UPDATE topics SET document_ids = ? WHERE id = ?",
                    (json.dumps(left), tid),
                )
        con.commit()
    finally:
        con.close()
    return {"ok": True}


def rpc_extract_entities(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(_param(params, "sqlitePath", "sqlite_path"))
    document_id = str(_param(params, "documentId", "document_id"))
    app_data = str(_param(params, "app_data", "appData") or "")
    extra_labels = [str(x).strip() for x in (_param(params, "labels") or []) if str(x).strip()]
    embed_model = _param(params, "embed_model", "embedModel")
    embed_model_s = str(embed_model) if embed_model else None

    try:
        con = sqlite3.connect(sqlite_path)
        try:
            card_row = con.execute(
                "SELECT type FROM document_cards WHERE document_id = ?",
                (document_id,),
            ).fetchone()
            card_type = str(card_row[0]) if card_row and card_row[0] else None
            labels = labels_for_document(card_type, extra_labels)

            # Clear prior mentions for this document before re-extract.
            old_ids = [
                str(r[0])
                for r in con.execute(
                    "SELECT DISTINCT entity_id FROM mentions WHERE document_id = ?",
                    (document_id,),
                ).fetchall()
            ]
            con.execute("DELETE FROM mentions WHERE document_id = ?", (document_id,))
            _refresh_entity_counts(con, old_ids)

            chunks = con.execute(
                """
                SELECT id, text, IFNULL(role, 'body')
                FROM chunks
                WHERE document_id = ?
                ORDER BY ordinal
                """,
                (document_id,),
            ).fetchall()

            raw_spans: list[dict[str, Any]] = []
            context_parts: list[str] = []
            for chunk_id, text, role in chunks:
                body = str(text or "")
                context_parts.append(body)
                if str(role or "body") == "references":
                    continue
                for start, _end, window in _windows(body):
                    for sp in extract_spans(
                        app_data, window, labels, threshold=_GLINER_THRESHOLD
                    ):
                        raw_spans.append(
                            {
                                "label": sp["label"],
                                "text": sp["text"],
                                "start": int(sp["start"]) + start,
                                "end": int(sp["end"]) + start,
                                "score": sp["score"],
                                "chunk_id": str(chunk_id),
                                "document_id": document_id,
                            }
                        )

            merged = merge_within_doc(raw_spans, context_text="\n".join(context_parts))
            mention_rows = 0
            entity_ids: set[str] = set()
            for it in merged:
                label = str(it["label"])
                name = str(it["name"])
                normalized = str(it["normalized"])
                eid = _entity_id(label.lower(), normalized)
                entity_ids.add(eid)
                row = con.execute(
                    "SELECT id, name FROM entities WHERE id = ?", (eid,)
                ).fetchone()
                if row:
                    # Prefer longer display name.
                    if len(name) > len(str(row[1] or "")):
                        con.execute(
                            "UPDATE entities SET name = ?, label = ? WHERE id = ?",
                            (name, label, eid),
                        )
                else:
                    # Also check by (label, normalized) in case of id drift.
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
                # Alias original surface if different.
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
            merged_n = _cross_doc_embed_merge(con, embed_model_s)
            con.commit()
            return {
                "ok": True,
                "mentions": mention_rows,
                "entities": len(entity_ids),
                "merged": merged_n,
            }
        finally:
            con.close()
    finally:
        unload_gliner()


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


def _mean_pool_doc_vectors(
    sqlite_path: str, doc_ids: list[str]
) -> np.ndarray | None:
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


def _mock_recompute(docs: list[dict[str, Any]]) -> dict[str, Any]:
    """Deterministic fake links/topics/points for tests."""
    ids = [str(d["id"]) for d in docs]
    links: list[dict[str, Any]] = []
    # Mentions from title/alias logic still applied by caller; add deterministic similar.
    for i, sid in enumerate(ids):
        for j, tid in enumerate(ids):
            if i >= j:
                continue
            # Fake weight from id order.
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
                if len([x for x in links if x["source_id"] == sid and x["kind"] == "similar"]) >= _SIMILAR_TOP_K:
                    break
    topics = []
    if ids:
        topics.append(
            {
                "id": "topic_mock_0",
                "name": "Mock topic",
                "document_ids": ids[: min(len(ids), 15)],
            }
        )
    points = []
    for i, did in enumerate(ids):
        points.append(
            {
                "document_id": did,
                "x": float(i),
                "y": float(i % 3),
                "topic_id": "topic_mock_0" if ids else None,
            }
        )
    return {"links": links, "topics": topics, "points": points}


def _name_topics(
    background_model: str | None,
    clusters: list[list[dict[str, Any]]],
) -> list[str]:
    names: list[str] = []
    if mock_models_enabled() or not background_model:
        for i, _ in enumerate(clusters):
            names.append(f"Topic {i + 1}")
        return names
    # One call for all clusters.
    blocks: list[str] = []
    for i, docs in enumerate(clusters):
        lines = []
        for d in docs[:12]:
            title = str(d.get("title") or d.get("id") or "")
            summ = "; ".join(d.get("summary") or [])[:240]
            lines.append(f"- {title}" + (f" :: {summ}" if summ else ""))
        blocks.append(f"Cluster {i + 1}:\n" + "\n".join(lines))
    prompt = (
        "Name each cluster with a short 2-5 word topic label. "
        "Reply with one label per line, same order, no numbering.\n\n"
        + "\n\n".join(blocks)
    )
    try:
        content = chat(
            background_model,
            [{"role": "user", "content": prompt}],
            think=False,
        )
    except Exception:
        return [f"Topic {i + 1}" for i in range(len(clusters))]
    lines = [ln.strip(" -•\t") for ln in content.splitlines() if ln.strip()]
    while len(lines) < len(clusters):
        lines.append(f"Topic {len(lines) + 1}")
    return lines[: len(clusters)]


def rpc_recompute_links(params: dict[str, Any]) -> dict[str, Any]:
    sqlite_path = str(_param(params, "sqlitePath", "sqlite_path"))
    _app_data = str(_param(params, "app_data", "appData") or "")
    del _app_data  # reserved for future local models
    embed_model = str(
        _param(params, "embed_model", "embedModel") or "qwen3-embedding:0.6b"
    )
    background_model = _param(params, "background_model", "backgroundModel")
    background_model_s = str(background_model) if background_model else None

    con = sqlite3.connect(sqlite_path)
    try:
        docs = _load_docs_for_links(con)
        con.execute("DELETE FROM document_links")
        con.execute("DELETE FROM topics")
        con.execute("DELETE FROM document_points")

        mention_links = find_mentions_links(docs)
        for sid, tid, evid, weight in mention_links:
            con.execute(
                """
                INSERT INTO document_links(
                  source_id, target_id, kind, weight, evidence_chunk_id
                ) VALUES (?, ?, 'mentions', ?, ?)
                """,
                (sid, tid, weight, evid),
            )

        doc_ids = [str(d["id"]) for d in docs]
        similar_count = 0
        topic_count = 0
        point_count = 0

        if mock_models_enabled() or mock_embed_enabled():
            fake = _mock_recompute(docs)
            # Still compute real mentions (already written); add mock similar/topics/points.
            for link in fake["links"]:
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
            for topic in fake["topics"]:
                con.execute(
                    "INSERT INTO topics(id, name, document_ids) VALUES (?, ?, ?)",
                    (topic["id"], topic["name"], json.dumps(topic["document_ids"])),
                )
                topic_count += 1
            for pt in fake["points"]:
                con.execute(
                    """
                    INSERT INTO document_points(document_id, x, y, topic_id)
                    VALUES (?, ?, ?, ?)
                    """,
                    (pt["document_id"], pt["x"], pt["y"], pt["topic_id"]),
                )
                point_count += 1
        else:
            vecs = _mean_pool_doc_vectors(sqlite_path, doc_ids)
            if vecs is not None and len(doc_ids) >= 2:
                for sid, tid, weight in similar_links(doc_ids, vecs):
                    con.execute(
                        """
                        INSERT INTO document_links(
                          source_id, target_id, kind, weight, evidence_chunk_id
                        ) VALUES (?, ?, 'similar', ?, NULL)
                        """,
                        (sid, tid, weight),
                    )
                    similar_count += 1

                clusters_idx = agglomerative_clusters(vecs)
                cluster_docs = [[docs[i] for i in idxs] for idxs in clusters_idx]
                names = _name_topics(background_model_s, cluster_docs)
                topic_of_doc: dict[str, str] = {}
                for name, idxs in zip(names, clusters_idx):
                    tid = f"topic_{uuid.uuid4().hex[:12]}"
                    member_ids = [doc_ids[i] for i in idxs]
                    con.execute(
                        "INSERT INTO topics(id, name, document_ids) VALUES (?, ?, ?)",
                        (tid, name[:120], json.dumps(member_ids)),
                    )
                    topic_count += 1
                    for mid in member_ids:
                        topic_of_doc[mid] = tid

                pts = pca_points(vecs)
                for i, did in enumerate(doc_ids):
                    x = float(pts[i, 0]) if i < len(pts) else 0.0
                    y = float(pts[i, 1]) if i < len(pts) else 0.0
                    con.execute(
                        """
                        INSERT INTO document_points(document_id, x, y, topic_id)
                        VALUES (?, ?, ?, ?)
                        """,
                        (did, x, y, topic_of_doc.get(did)),
                    )
                    point_count += 1
            elif doc_ids:
                for did in doc_ids:
                    con.execute(
                        """
                        INSERT INTO document_points(document_id, x, y, topic_id)
                        VALUES (?, 0, 0, NULL)
                        """,
                        (did,),
                    )
                    point_count += 1

        con.commit()
        return {
            "ok": True,
            "mentionsLinks": len(mention_links),
            "similarLinks": similar_count,
            "topics": topic_count,
            "points": point_count,
        }
    finally:
        con.close()
