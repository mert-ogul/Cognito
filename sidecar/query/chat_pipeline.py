"""Scoped hybrid retrieval: vector + keyword RRF → label boost → rerank → notFound."""

from __future__ import annotations

import json
import logging
import re
import sqlite3
import time
from pathlib import Path
from typing import Any

import sqlite_vec

from common.model_store import rerank_with_budget
from common.ollama import embed_prefixes, embed_text
from common.sqlite_vec_util import (
    MOCK_EMBED_DIM,
    connect_sqlite,
    connect_with_vec,
    mock_embed_enabled,
    table_dimension,
)
from common.text_norm import normalize_search_text
from ingestion.chunking import TEXT_QUALITY_MIN

log = logging.getLogger("cognito.sidecar")

_VECTOR_K = 30
_KEYWORD_K = 30
_RRF_K = 60
_RERANK_POOL = 30
_KEEP = 5
# Codes/numbers like 1202, AS-204, MSC-00171 get their own BM25 list in RRF.
_RARE_TOKEN_RE = re.compile(
    r"(?<![A-Za-z0-9])((?:[A-Za-z]+-)?\d[A-Za-z0-9.-]{2,}|\d{3,})(?![A-Za-z0-9])"
)
# Between the highest unanswerable score (0.0084) and the lowest answerable one (0.0316) on eval/questions.jsonl.
NOT_FOUND_SCORE = 0.015
_HISTORY_TURN_CAP = 2000
_HISTORY_TOTAL_CAP = 4000
_CITE_MARK_RE = re.compile(r"\[\d+\]")
_FTS_WORD_RE = re.compile(r"[^\W_]+(?:'[^\W_]+)?")
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
_LABEL_MENTION_RE = re.compile(
    r"(?:"
    r"(?:equations?|eq\.?)\s*\(?\s*(\d+)\s*\)?"
    r"|(?:tables?)\s+(\d+)"
    r"|(?:figures?|fig\.?)\s+(\d+)"
    r")",
    re.IGNORECASE,
)
_FRONT_SECTION_RE = re.compile(
    r"^(?:abstract|introduction)\b",
    re.IGNORECASE,
)
_MD_HEADING_RE = re.compile(r"^#{1,6}\s+\S")
_ALL_CAPS_HEADING_RE = re.compile(r"^[A-Z][A-Z0-9 ,.\-]{1,48}$")


class EmbeddingDimMismatch(Exception):
    """Query embedding length does not match the stored index."""


def _indexed_chunk_count(sqlite_path: str) -> int:
    try:
        con = connect_sqlite(sqlite_path)
        try:
            row = con.execute("SELECT COUNT(*) FROM chunks").fetchone()
            return int(row[0]) if row else 0
        finally:
            con.close()
    except Exception:
        return 0


def _norm_token(w: str) -> str:
    t = (w or "").lower()
    if t.endswith("'s"):
        t = t[:-2]
    return t


def _significant_words(text: str) -> list[str]:
    out: list[str] = []
    for m in _FTS_WORD_RE.finditer(text or ""):
        t = _norm_token(m.group(0))
        if len(t) < 2 or t in _FTS_STOPWORDS:
            continue
        out.append(t)
    return out


def build_fts_query(text: str) -> str:
    """Build an FTS5 OR-query from question words (quoted; short stopwords dropped)."""
    parts: list[str] = []
    for m in _FTS_WORD_RE.finditer(text or ""):
        w = m.group(0)
        if w.isdigit() or len(w) >= 2:
            if w.lower() in _FTS_STOPWORDS:
                continue
            parts.append('"' + w.replace('"', '""') + '"')
    return " OR ".join(parts)


def extract_rare_tokens(text: str) -> list[str]:
    """Number/code tokens of 3+ chars (1202, AS-204, MSC-00171)."""
    seen: set[str] = set()
    out: list[str] = []
    for m in _RARE_TOKEN_RE.finditer(text or ""):
        tok = m.group(1)
        key = tok.lower()
        if key in seen:
            continue
        seen.add(key)
        out.append(tok)
    return out


def _chunks_have_text_quality(con: sqlite3.Connection) -> bool:
    try:
        cols = con.execute("PRAGMA table_info(chunks)").fetchall()
    except Exception:
        return False
    return any(str(r[1]) == "text_quality" for r in cols)


def _quality_sql(alias: str = "c") -> str:
    return f" AND ({alias}.text_quality IS NULL OR {alias}.text_quality >= {TEXT_QUALITY_MIN})"


def reciprocal_rank_fusion(
    ranked_lists: list[list[dict[str, Any]]], k: int = _RRF_K
) -> list[dict[str, Any]]:
    scores: dict[str, float] = {}
    by_id: dict[str, dict[str, Any]] = {}
    for lst in ranked_lists:
        for rank, item in enumerate(lst, start=1):
            cid = str(item["chunk_id"])
            role = str(item.get("role") or "body")
            weight = 0.5 if role == "references" else 1.0
            scores[cid] = scores.get(cid, 0.0) + weight / (k + rank)
            by_id[cid] = item
    ordered = sorted(scores.keys(), key=lambda cid: (-scores[cid], cid))
    return [by_id[cid] for cid in ordered]


def _parse_labels(raw: Any) -> list[str]:
    if isinstance(raw, list):
        return [str(x) for x in raw]
    if not raw:
        return []
    try:
        parsed = json.loads(raw)
    except (TypeError, json.JSONDecodeError):
        return []
    if isinstance(parsed, list):
        return [str(x) for x in parsed]
    return []


def _parse_regions(raw: Any) -> list[dict[str, Any]] | None:
    if isinstance(raw, list):
        return [x for x in raw if isinstance(x, dict)]
    if not raw:
        return None
    try:
        parsed = json.loads(raw)
    except (TypeError, json.JSONDecodeError):
        return None
    if isinstance(parsed, list):
        return [x for x in parsed if isinstance(x, dict)]
    return None


def _chunk_row_dict(r: tuple[Any, ...], chunk_id: str) -> dict[str, Any]:
    did, text, ps, pe, title, dpath = r[:6]
    role = str(r[6]) if len(r) > 6 and r[6] else "body"
    labels = _parse_labels(r[7] if len(r) > 7 else None)
    regions = _parse_regions(r[8] if len(r) > 8 else None)
    ordinal = r[9] if len(r) > 9 else None
    out: dict[str, Any] = {
        "chunk_id": chunk_id,
        "document_id": did,
        "text": text,
        "page_start": ps,
        "page_end": pe,
        "title": title,
        "path": dpath,
        "role": role,
        "labels": labels,
        "regions": regions,
    }
    if ordinal is not None:
        out["ordinal"] = int(ordinal)
    return out


def _doc_id_clause(document_ids: list[str] | None, params: list[Any]) -> str:
    if not document_ids:
        return ""
    ph = ",".join(["?"] * len(document_ids))
    params.extend(document_ids)
    return f" AND c.document_id IN ({ph})"


def vector_top_k(
    sqlite_path: str,
    embed_model: str,
    query: str,
    k: int,
    document_ids: list[str] | None = None,
) -> list[dict[str, Any]]:
    con = connect_with_vec(sqlite_path)
    dim = table_dimension(con)
    if dim is None:
        con.close()
        return []

    q_prefix, _ = embed_prefixes(embed_model)
    embed_input = f"{q_prefix}{query}"
    try:
        qv = list(
            embed_text(embed_model, embed_input)
            if not mock_embed_enabled()
            else [0.0] * MOCK_EMBED_DIM
        )
    except Exception:
        con.close()
        return []
    if len(qv) != dim:
        con.close()
        raise EmbeddingDimMismatch(f"query embedding has {len(qv)} values; index uses {dim}")
    fetch_k = k
    if document_ids:
        fetch_k = max(k * 20, k)
    qblob = sqlite_vec.serialize_float32(list(qv))
    rows = con.execute(
        """
        SELECT chunk_id, distance
        FROM chunk_embeddings
        WHERE embedding MATCH ?
        AND k = ?
        """,
        (qblob, fetch_k),
    ).fetchall()
    allow = set(document_ids) if document_ids else None
    quality_ok = _chunks_have_text_quality(con)
    out: list[dict[str, Any]] = []
    for chunk_id, _dist in rows:
        r = con.execute(
            """
            SELECT c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                   c.role, c.labels, c.regions, c.ordinal, c.text_quality
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.id = ? AND d.status = 'indexed'
            """
            if quality_ok
            else """
            SELECT c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                   c.role, c.labels, c.regions, c.ordinal, NULL
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.id = ? AND d.status = 'indexed'
            """,
            (chunk_id,),
        ).fetchone()
        if not r:
            continue
        if allow is not None and str(r[0]) not in allow:
            continue
        tq = r[10]
        if tq is not None and float(tq) < TEXT_QUALITY_MIN:
            continue
        out.append(_chunk_row_dict(r[:10], chunk_id))
        if len(out) >= k:
            break
    con.close()
    return out


def keyword_top_k(
    sqlite_path: str,
    text: str,
    k: int,
    document_ids: list[str] | None = None,
) -> list[dict[str, Any]]:
    fts_q = build_fts_query(text)
    if not fts_q or k <= 0:
        return []
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return []
    params: list[Any] = [fts_q]
    scope_sql = _doc_id_clause(document_ids, params)
    quality_sql = _quality_sql("c") if _chunks_have_text_quality(con) else ""
    # Over-fetch then trim so quality filter does not shrink the list silently.
    params.append(max(k * 3, k))
    try:
        rows = con.execute(
            f"""
            SELECT c.id, c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                   c.role, c.labels, c.regions, c.ordinal
            FROM chunks_fts
            JOIN chunks c ON c.rowid = chunks_fts.rowid
            JOIN documents d ON d.id = c.document_id
            WHERE chunks_fts MATCH ?
              AND d.status = 'indexed'
              {scope_sql}
              {quality_sql}
            ORDER BY bm25(chunks_fts)
            LIMIT ?
            """,
            params,
        ).fetchall()
    except Exception:
        return []
    finally:
        con.close()
    out: list[dict[str, Any]] = []
    for chunk_id, did, body, ps, pe, title, dpath, role, labels, regions, ordinal in rows:
        out.append(
            {
                "chunk_id": chunk_id,
                "document_id": did,
                "text": body,
                "page_start": ps,
                "page_end": pe,
                "title": title,
                "path": dpath,
                "role": role or "body",
                "labels": _parse_labels(labels),
                "regions": _parse_regions(regions),
                "ordinal": int(ordinal) if ordinal is not None else 0,
            }
        )
        if len(out) >= k:
            break
    return out


def exact_token_top_k(
    sqlite_path: str,
    text: str,
    k: int,
    document_ids: list[str] | None = None,
) -> list[dict[str, Any]]:
    """BM25 rank of chunks that contain rare number/code tokens from the question."""
    tokens = extract_rare_tokens(text)
    if not tokens or k <= 0:
        return []
    fts_q = " OR ".join('"' + t.replace('"', '""') + '"' for t in tokens)
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return []
    params: list[Any] = [fts_q]
    scope_sql = _doc_id_clause(document_ids, params)
    quality_sql = _quality_sql("c") if _chunks_have_text_quality(con) else ""
    params.append(max(k * 3, k))
    try:
        rows = con.execute(
            f"""
            SELECT c.id, c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                   c.role, c.labels, c.regions, c.ordinal
            FROM chunks_fts
            JOIN chunks c ON c.rowid = chunks_fts.rowid
            JOIN documents d ON d.id = c.document_id
            WHERE chunks_fts MATCH ?
              AND d.status = 'indexed'
              {scope_sql}
              {quality_sql}
            ORDER BY bm25(chunks_fts)
            LIMIT ?
            """,
            params,
        ).fetchall()
    except Exception:
        return []
    finally:
        con.close()
    # Keep only chunks that literally contain at least one rare token.
    lowered = [t.lower() for t in tokens]
    out: list[dict[str, Any]] = []
    for chunk_id, did, body, ps, pe, title, dpath, role, labels, regions, ordinal in rows:
        body_l = str(body or "").lower()
        if not any(tok in body_l for tok in lowered):
            continue
        out.append(
            {
                "chunk_id": chunk_id,
                "document_id": did,
                "text": body,
                "page_start": ps,
                "page_end": pe,
                "title": title,
                "path": dpath,
                "role": role or "body",
                "labels": _parse_labels(labels),
                "regions": _parse_regions(regions),
                "ordinal": int(ordinal) if ordinal is not None else 0,
            }
        )
        if len(out) >= k:
            break
    return out


def author_lines_from_front(text: str, title: str) -> list[str]:
    """Lines between the title (or start) and Abstract / Introduction / a heading."""
    lines = (text or "").splitlines()
    start = 0
    title_s = (title or "").strip()
    if title_s:
        title_l = title_s.lower()
        for i, ln in enumerate(lines):
            s = ln.strip()
            if not s:
                continue
            if s.lower() == title_l or title_l in s.lower():
                start = i + 1
                break
    out: list[str] = []
    for ln in lines[start:]:
        s = ln.strip()
        if not s:
            continue
        if _FRONT_SECTION_RE.match(s) or _MD_HEADING_RE.match(s):
            break
        if _ALL_CAPS_HEADING_RE.match(s) and " " in s:
            break
        out.append(s)
    return out


def _parse_json_list(raw: Any) -> list[Any]:
    if isinstance(raw, list):
        return list(raw)
    if not raw:
        return []
    try:
        parsed = json.loads(raw)
    except (TypeError, json.JSONDecodeError):
        return []
    if isinstance(parsed, list):
        return list(parsed)
    return []


def _person_name(item: Any) -> str:
    if isinstance(item, dict):
        return str(item.get("name") or "")
    return str(item or "")


def _alias_tokens(alias: str) -> list[str]:
    return [_norm_token(m.group(0)) for m in _FTS_WORD_RE.finditer(alias or "")]


_PROFILE_PAGE1_CHARS = 1500
_SCOPE_SCORE_MARGIN = 1.5


def build_document_profile_text(
    *,
    title: str,
    path: str,
    people: list[Any] | None = None,
    organizations: list[Any] | None = None,
    aliases: list[Any] | None = None,
    page1_text: str = "",
) -> str:
    parts: list[str] = []
    if (title or "").strip():
        parts.append(title.strip())
    stem = Path(path or "").stem.replace("_", " ").replace("-", " ").strip()
    if stem:
        parts.append(stem)
    for person in people or []:
        name = _person_name(person).strip()
        if name:
            parts.append(name)
    for org in organizations or []:
        s = str(org or "").strip()
        if s:
            parts.append(s)
    for alias in aliases or []:
        s = str(alias or "").strip()
        if s:
            parts.append(s)
    head = normalize_search_text(page1_text or "")[:_PROFILE_PAGE1_CHARS]
    if head.strip():
        parts.append(head)
    return normalize_search_text("\n".join(parts))


def rebuild_document_profile(sqlite_path: str, document_id: str) -> None:
    """Upsert documents_profile (+ FTS triggers) for one document."""
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return
    try:
        if not _table_exists(con, "documents_profile"):
            return
        row = con.execute(
            "SELECT title, path FROM documents WHERE id = ?",
            (document_id,),
        ).fetchone()
        if not row:
            return
        title, path = str(row[0] or ""), str(row[1] or "")
        people: list[Any] = []
        organizations: list[Any] = []
        aliases: list[Any] = []
        if _table_exists(con, "document_cards"):
            card = con.execute(
                "SELECT people, organizations, aliases FROM document_cards WHERE document_id = ?",
                (document_id,),
            ).fetchone()
            if card:
                people = _card_people(card[0])
                organizations = _card_string_list(card[1])
                aliases = _card_string_list(card[2])
        page_rows = con.execute(
            """
            SELECT text FROM chunks
            WHERE document_id = ?
              AND (page_start IS NULL OR page_start <= 1)
            ORDER BY ordinal
            """,
            (document_id,),
        ).fetchall()
        page1 = "\n".join(str(r[0] or "") for r in page_rows)
        profile = build_document_profile_text(
            title=title,
            path=path,
            people=people,
            organizations=organizations,
            aliases=aliases,
            page1_text=page1,
        )
        con.execute(
            """
            INSERT INTO documents_profile(document_id, profile) VALUES (?, ?)
            ON CONFLICT(document_id) DO UPDATE SET profile = excluded.profile
            """,
            (document_id, profile),
        )
        con.commit()
    except Exception:
        return
    finally:
        con.close()


def _profile_term_hits(question: str, profile: str) -> int:
    q_terms = set(_significant_words(question))
    if not q_terms:
        return 0
    p_terms = set(_significant_words(profile))
    return len(q_terms & p_terms)


def _scope_anchor_hits(
    question: str,
    title: str,
    path: str,
    *,
    people: list[Any] | None = None,
    organizations: list[Any] | None = None,
    aliases: list[Any] | None = None,
) -> int:
    """Overlap with title, file stem, people, orgs, aliases — not page-1 body."""
    q_terms = set(_significant_words(question))
    if not q_terms:
        return 0
    parts: list[str] = [str(title or "")]
    stem = Path(path or "").stem.replace("_", " ").replace("-", " ")
    if stem:
        parts.append(stem)
    for person in people or []:
        name = _person_name(person).strip()
        if name:
            parts.append(name)
    for org in organizations or []:
        s = str(org or "").strip()
        if s:
            parts.append(s)
    for alias in aliases or []:
        s = str(alias or "").strip()
        if s:
            parts.append(s)
    anchors = set(_significant_words(" ".join(parts)))
    return len(q_terms & anchors)


def match_document_profiles(
    sqlite_path: str, question: str, *, limit: int = 3
) -> list[dict[str, Any]]:
    """Top document profiles by BM25; each entry includes score and termHits."""
    q = normalize_search_text(question or "")
    fts_q = build_fts_query(q)
    if not fts_q:
        return []
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return []
    try:
        if not _table_exists(con, "documents_fts"):
            return []
        rows = con.execute(
            """
            SELECT p.document_id, d.title, d.path, p.profile,
                   bm25(documents_fts) AS score
            FROM documents_fts
            JOIN documents_profile p ON p.rowid = documents_fts.rowid
            JOIN documents d ON d.id = p.document_id
            WHERE documents_fts MATCH ?
              AND d.status = 'indexed'
            ORDER BY bm25(documents_fts)
            LIMIT ?
            """,
            (fts_q, max(limit, 3)),
        ).fetchall()
        has_cards = _table_exists(con, "document_cards")
        out: list[dict[str, Any]] = []
        for did, title, path, profile, bm25_score in rows:
            people: list[Any] = []
            organizations: list[Any] = []
            aliases: list[Any] = []
            if has_cards:
                card = con.execute(
                    "SELECT people, organizations, aliases FROM document_cards WHERE document_id = ?",
                    (did,),
                ).fetchone()
                if card:
                    people = _card_people(card[0])
                    organizations = _card_string_list(card[1])
                    aliases = _card_string_list(card[2])
            # FTS5 bm25: lower is better; expose positive relevance for margins.
            relevance = -float(bm25_score)
            out.append(
                {
                    "documentId": str(did),
                    "title": str(title or ""),
                    "path": str(path or ""),
                    "score": relevance,
                    "termHits": _profile_term_hits(q, str(profile or "")),
                    "anchorHits": _scope_anchor_hits(
                        q,
                        str(title or ""),
                        str(path or ""),
                        people=people,
                        organizations=organizations,
                        aliases=aliases,
                    ),
                    "profile": str(profile or ""),
                }
            )
        # Prefer richer question-term overlap, then BM25 relevance.
        out.sort(key=lambda m: (-int(m["termHits"]), -float(m["score"]), m["documentId"]))
        return out[:limit]
    except Exception:
        return []
    finally:
        con.close()


def resolve_scope(
    sqlite_path: str,
    question: str,
    *,
    explicit_ids: list[str] | None = None,
) -> dict[str, Any]:
    """Pick a document from documents_fts when the lead is clear; else none."""
    matches = match_document_profiles(sqlite_path, question, limit=3)
    profile_matches = [
        {
            "documentId": m["documentId"],
            "title": m["title"],
            "path": m["path"],
            "score": m["score"],
            "termHits": m["termHits"],
        }
        for m in matches
    ]
    if explicit_ids:
        ids = [str(x) for x in explicit_ids if x]
        return {
            "scopeIds": ids,
            "scopeTitle": None,
            "scopeChosen": False,
            "scopeReason": "explicit",
            "profileMatches": profile_matches,
        }
    if not matches:
        return {
            "scopeIds": [],
            "scopeTitle": None,
            "scopeChosen": False,
            "scopeReason": "no_profile_match",
            "profileMatches": [],
        }
    best = matches[0]
    runner = matches[1] if len(matches) > 1 else None
    best_hits = int(best.get("termHits") or 0)
    anchor_hits = int(best.get("anchorHits") or 0)
    # Notes/question lists can BM25-match hard while their title does not name the topic.
    if best_hits < 2 or anchor_hits < 1:
        return {
            "scopeIds": [],
            "scopeTitle": None,
            "scopeChosen": False,
            "scopeReason": "ambiguous_or_weak",
            "profileMatches": profile_matches,
        }
    if runner is None:
        clear = True
    elif best_hits > int(runner.get("termHits") or 0):
        clear = True
    else:
        runner_score = float(runner.get("score") or 0.0)
        clear = float(best["score"]) >= _SCOPE_SCORE_MARGIN * runner_score
    if clear:
        return {
            "scopeIds": [best["documentId"]],
            "scopeTitle": best["title"] or None,
            "scopeChosen": True,
            "scopeReason": "profile_bm25",
            "profileMatches": profile_matches,
        }
    return {
        "scopeIds": [],
        "scopeTitle": None,
        "scopeChosen": False,
        "scopeReason": "ambiguous_or_weak",
        "profileMatches": profile_matches,
    }


def question_label_mentions(question: str) -> list[str]:
    found: list[str] = []
    seen: set[str] = set()
    for m in _LABEL_MENTION_RE.finditer(question or ""):
        if m.group(1) is not None:
            label = f"({m.group(1)})"
        elif m.group(2) is not None:
            label = f"Table {m.group(2)}"
        else:
            label = f"Figure {m.group(3)}"
        key = label.lower()
        if key in seen:
            continue
        seen.add(key)
        found.append(label)
    return found


def _label_norm(label: str) -> str:
    s = re.sub(r"\s+", " ", (label or "").strip().lower())
    s = s.replace("fig.", "figure")
    return s


def _chunk_has_label(chunk: dict[str, Any], wanted: list[str]) -> bool:
    if not wanted:
        return False
    have = {_label_norm(x) for x in (chunk.get("labels") or [])}
    for w in wanted:
        wn = _label_norm(w)
        if wn in have:
            return True
        # "(2)" vs "2" / "equation 2" flexibility
        bare = wn.strip("()")
        if any(_label_norm(h).strip("()") == bare or wn == _label_norm(h) for h in have):
            return True
    return False


def apply_label_boost(chunks: list[dict[str, Any]], labels: list[str]) -> list[dict[str, Any]]:
    if not labels or not chunks:
        return chunks
    hit = [c for c in chunks if _chunk_has_label(c, labels)]
    miss = [c for c in chunks if not _chunk_has_label(c, labels)]
    return hit + miss


def label_chunks_for_documents(
    sqlite_path: str,
    document_ids: list[str],
    labels: list[str],
) -> list[dict[str, Any]]:
    """Chunks in the given documents that carry any of the question labels."""
    if not document_ids or not labels:
        return []
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return []
    try:
        ph = ",".join(["?"] * len(document_ids))
        rows = con.execute(
            f"""
            SELECT c.id, c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                   c.role, c.labels, c.regions, c.ordinal
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.document_id IN ({ph})
              AND d.status = 'indexed'
              AND c.labels IS NOT NULL
              AND c.labels != '[]'
              AND c.labels != ''
            ORDER BY c.ordinal
            """,
            list(document_ids),
        ).fetchall()
    except Exception:
        return []
    finally:
        con.close()
    out: list[dict[str, Any]] = []
    for chunk_id, did, body, ps, pe, title, dpath, role, lab_raw, regions, ordinal in rows:
        chunk = {
            "chunk_id": chunk_id,
            "document_id": did,
            "text": body,
            "page_start": ps,
            "page_end": pe,
            "title": title,
            "path": dpath,
            "role": role or "body",
            "labels": _parse_labels(lab_raw),
            "regions": _parse_regions(regions),
            "ordinal": int(ordinal) if ordinal is not None else 0,
        }
        if _chunk_has_label(chunk, labels):
            out.append(chunk)
    return out


def _merge_chunks_by_id(
    primary: list[dict[str, Any]], extra: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    seen = {str(c.get("chunk_id")) for c in primary}
    out = list(primary)
    for c in extra:
        cid = str(c.get("chunk_id") or "")
        if not cid or cid in seen:
            continue
        seen.add(cid)
        out.append(c)
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


def _table_exists(con: sqlite3.Connection, name: str) -> bool:
    row = con.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
        (name,),
    ).fetchone()
    return row is not None


def _card_string_list(raw: Any) -> list[str]:
    out: list[str] = []
    for item in _parse_json_list(raw):
        s = str(item).strip() if item is not None else ""
        if s:
            out.append(s)
    return out


def _card_people(raw: Any) -> list[Any]:
    out: list[Any] = []
    for item in _parse_json_list(raw):
        if isinstance(item, dict):
            name = str(item.get("name") or "").strip()
            if not name:
                continue
            role = item.get("role")
            entry: dict[str, Any] = {"name": name}
            if role is not None and str(role).strip():
                entry["role"] = str(role)
            out.append(entry)
        else:
            s = str(item or "").strip()
            if s:
                out.append(s)
    return out


def _citation_from_chunk(c: dict[str, Any], index: int) -> dict[str, Any]:
    body = str(c.get("text") or c.get("snippet") or "")
    out: dict[str, Any] = {
        "documentId": c.get("document_id") or c.get("documentId"),
        "title": c.get("title") or "",
        "path": c.get("path") or "",
        "pageStart": c.get("page_start", c.get("pageStart")),
        "pageEnd": c.get("page_end", c.get("pageEnd")),
        "chunkId": c.get("chunk_id") or c.get("chunkId"),
        "snippet": body[:1200],
        "text": body,
        "nodeIds": c.get("nodeIds") or c.get("node_ids") or [],
        "index": index,
        "regions": c.get("regions"),
    }
    labels = c.get("labels")
    if labels is not None:
        out["labels"] = labels
    ordinal = c.get("ordinal")
    if ordinal is not None:
        out["ordinal"] = int(ordinal)
    return out


def _empty_result(
    *,
    scope_ids: list[str],
    scope_title: str | None,
    scope_chosen: bool,
    not_found: bool = True,
    query_embedding_failed: bool = False,
    profile_matches: list[dict[str, Any]] | None = None,
    suggestions: list[dict[str, Any]] | None = None,
    trace: dict[str, Any] | None = None,
    search_text: str | None = None,
) -> dict[str, Any]:
    out: dict[str, Any] = {
        "retrievalMode": "vector_only",
        "citations": [],
        "candidates": [],
        "notFound": not_found,
        "bestScore": 0.0,
        "scopeIds": list(scope_ids),
        "scopeTitle": scope_title,
        "scopeChosen": scope_chosen,
        "profileMatches": list(profile_matches or []),
        "suggestions": list(suggestions or []),
        "entityChunks": [],
        "retrievalMs": 0,
        "rerankMs": 0,
        "rerankScored": 0,
        "searchText": search_text or "",
        "trace": trace or {},
    }
    if query_embedding_failed:
        out["queryEmbeddingFailed"] = True
    return out


def _build_trace(
    *,
    normalised_question: str,
    profile_matches: list[dict[str, Any]],
    scope_ids: list[str],
    scope_reason: str,
    vector_n: int,
    keyword_n: int,
    label_n: int,
    reranked: list[tuple[dict[str, Any], float]] | None = None,
    second_pass: bool = False,
) -> dict[str, Any]:
    top10: list[dict[str, Any]] = []
    for c, score in (reranked or [])[:10]:
        top10.append(
            {
                "chunkId": c.get("chunk_id") or c.get("chunkId"),
                "documentId": c.get("document_id") or c.get("documentId"),
                "title": c.get("title"),
                "pageStart": c.get("page_start") if "page_start" in c else c.get("pageStart"),
                "score": float(score),
            }
        )
    return {
        "normalisedQuestion": normalised_question,
        "profileMatches": profile_matches,
        "scopeIds": list(scope_ids),
        "scopeReason": scope_reason,
        "candidateCounts": {
            "vector": vector_n,
            "keyword": keyword_n,
            "label": label_n,
        },
        "topReranked": top10,
        "secondPass": second_pass,
    }


def _log_trace(trace: dict[str, Any]) -> None:
    log.debug("retrieval_trace %s", json.dumps(trace, default=str))


def _suggestions_from_profiles(
    matches: list[dict[str, Any]], *, n: int = 2
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for m in matches[:n]:
        title = str(m.get("title") or "").strip() or Path(str(m.get("path") or "")).stem
        out.append(
            {
                "documentId": m["documentId"],
                "title": title,
                "label": f"Search in {title}",
            }
        )
    return out


def _retrieve_pool(
    sqlite_path: str,
    embed_model: str,
    search_text: str,
    top_k: int,
    scope_filter: list[str] | None,
    labels: list[str],
    profile_matches: list[dict[str, Any]],
    *,
    inject_label_from_profiles: bool,
) -> tuple[list[dict[str, Any]], int, int, int, list[str]]:
    entity_chunk_ids = match_entity_mention_chunks(sqlite_path, search_text)
    entity_chunks = _chunks_by_ids(sqlite_path, entity_chunk_ids)
    vector_chunks = vector_top_k(
        sqlite_path, embed_model, search_text, top_k, document_ids=scope_filter
    )
    keyword_chunks = keyword_top_k(sqlite_path, search_text, _KEYWORD_K, document_ids=scope_filter)
    exact_chunks = exact_token_top_k(
        sqlite_path, search_text, _KEYWORD_K, document_ids=scope_filter
    )
    label_extra: list[dict[str, Any]] = []
    if labels:
        if scope_filter:
            doc_ids = [str(x) for x in scope_filter]
        elif inject_label_from_profiles:
            doc_ids = [str(m["documentId"]) for m in profile_matches[:3]]
        else:
            doc_ids = []
        if doc_ids:
            label_extra = label_chunks_for_documents(sqlite_path, doc_ids, labels)
    ranked_lists: list[list[dict[str, Any]]] = [vector_chunks, keyword_chunks]
    if exact_chunks:
        ranked_lists.append(exact_chunks)
    if entity_chunks:
        ranked_lists.append(entity_chunks)
    fused = reciprocal_rank_fusion(ranked_lists)
    fused = _merge_chunks_by_id(fused, label_extra)
    boosted = apply_label_boost(fused, labels)
    pool = boosted[:_RERANK_POOL]
    return (
        pool,
        len(vector_chunks),
        len(keyword_chunks),
        len(label_extra),
        list(entity_chunk_ids),
    )


def _rerank_pool(
    app_data: str | None,
    search_text: str,
    pool: list[dict[str, Any]],
) -> tuple[list[tuple[dict[str, Any], float]], dict[str, Any]]:
    passages = [str(c.get("text") or c.get("snippet") or "") for c in pool]
    rr = rerank_with_budget(
        app_data or "",
        search_text,
        passages,
    )
    ranked = sorted(
        zip(pool, rr["scores"]),
        key=lambda pair: (
            -pair[1],
            str(pair[0].get("chunk_id") or pair[0].get("chunkId") or ""),
        ),
    )
    return ranked, rr


_ENTITY_DOC_CAP = 5


def _chunks_by_ids(sqlite_path: str, chunk_ids: list[str]) -> list[dict[str, Any]]:
    if not chunk_ids:
        return []
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return []
    try:
        quality_ok = _chunks_have_text_quality(con)
        out: list[dict[str, Any]] = []
        seen: set[str] = set()
        for cid in chunk_ids:
            if cid in seen:
                continue
            seen.add(cid)
            r = con.execute(
                """
                SELECT c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                       c.role, c.labels, c.regions, c.ordinal, c.text_quality
                FROM chunks c
                JOIN documents d ON d.id = c.document_id
                WHERE c.id = ? AND d.status = 'indexed'
                """
                if quality_ok
                else """
                SELECT c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                       c.role, c.labels, c.regions, c.ordinal, NULL
                FROM chunks c
                JOIN documents d ON d.id = c.document_id
                WHERE c.id = ? AND d.status = 'indexed'
                """,
                (cid,),
            ).fetchone()
            if r:
                tq = r[10]
                if tq is not None and float(tq) < TEXT_QUALITY_MIN:
                    continue
                out.append(_chunk_row_dict(r[:10], cid))
        return out
    except Exception:
        return []
    finally:
        con.close()


def _chunk_id_of(c: dict[str, Any]) -> str:
    return str(c.get("chunkId") or c.get("chunk_id") or "")


def _document_id_of(c: dict[str, Any]) -> str:
    return str(c.get("documentId") or c.get("document_id") or "")


def _labels_of(c: dict[str, Any]) -> list[str]:
    raw = c.get("labels")
    if isinstance(raw, list):
        return [str(x) for x in raw]
    return _parse_labels(raw)


def _ordinal_of(c: dict[str, Any], sqlite_path: str | None) -> int | None:
    raw = c.get("ordinal")
    if raw is not None:
        try:
            return int(raw)
        except (TypeError, ValueError):
            pass
    cid = _chunk_id_of(c)
    if not cid or not sqlite_path:
        return None
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return None
    try:
        row = con.execute("SELECT ordinal FROM chunks WHERE id = ?", (cid,)).fetchone()
        if row is None or row[0] is None:
            return None
        return int(row[0])
    except Exception:
        return None
    finally:
        con.close()


def _chunk_after_ordinal(sqlite_path: str, document_id: str, ordinal: int) -> dict[str, Any] | None:
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return None
    try:
        row = con.execute(
            """
            SELECT c.id, c.document_id, c.text, c.page_start, c.page_end, d.title, d.path,
                   c.role, c.labels, c.regions, c.ordinal
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.document_id = ? AND c.ordinal = ? AND d.status = 'indexed'
            """,
            (document_id, ordinal + 1),
        ).fetchone()
        if not row:
            return None
        return {
            "chunk_id": row[0],
            "document_id": row[1],
            "text": row[2],
            "page_start": row[3],
            "page_end": row[4],
            "title": row[5],
            "path": row[6],
            "role": row[7] or "body",
            "labels": _parse_labels(row[8]),
            "regions": _parse_regions(row[9]),
            "ordinal": int(row[10]) if row[10] is not None else ordinal + 1,
        }
    except Exception:
        return None
    finally:
        con.close()


def _pin_label_chunks(
    ranked: list[tuple[dict[str, Any], float]],
    labels: list[str],
    *,
    sqlite_path: str | None,
    matched_doc_ids: set[str],
) -> tuple[list[tuple[dict[str, Any], float]], bool]:
    """Move up to two labelled chunks (+ next ordinal) to the front after rerank."""
    if not labels or not ranked:
        return ranked, False

    hits: list[tuple[dict[str, Any], float]] = []
    for c, score in ranked:
        if _chunk_has_label({"labels": _labels_of(c)}, labels):
            hits.append((c, score))
            if len(hits) >= 2:
                break
    if not hits:
        return ranked, False

    pinned: list[tuple[dict[str, Any], float]] = []
    used: set[str] = set()
    covers = False
    for c, score in hits:
        cid = _chunk_id_of(c)
        if not cid or cid in used:
            continue
        pinned.append((c, score))
        used.add(cid)
        did = _document_id_of(c)
        if did and did in matched_doc_ids:
            covers = True
        if not sqlite_path or not did:
            continue
        ordinal = _ordinal_of(c, sqlite_path)
        if ordinal is None:
            continue
        nxt = _chunk_after_ordinal(sqlite_path, did, ordinal)
        if not nxt:
            continue
        nid = _chunk_id_of(nxt)
        if not nid or nid in used:
            continue
        pinned.append((_citation_from_chunk(nxt, 0), score))
        used.add(nid)

    rest = [(c, s) for c, s in ranked if _chunk_id_of(c) not in used]
    return pinned + rest, covers


def match_entity_mention_chunks(
    sqlite_path: str, question: str, *, doc_cap: int = _ENTITY_DOC_CAP
) -> list[str]:
    """Chunk ids for entities named in the question that appear in few documents."""
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return []
    try:
        if not _table_exists(con, "entities") or not _table_exists(con, "mentions"):
            return []
        q_words = {_norm_token(m.group(0)) for m in _FTS_WORD_RE.finditer(question or "")}
        if not q_words:
            return []
        entities = con.execute("SELECT id, name, normalized FROM entities").fetchall()
        matched: list[str] = []
        for eid, name, normalized in entities:
            surfaces = [str(name or ""), str(normalized or "")]
            if _table_exists(con, "entity_aliases"):
                for (alias,) in con.execute(
                    "SELECT alias FROM entity_aliases WHERE entity_id = ?",
                    (eid,),
                ).fetchall():
                    surfaces.append(str(alias or ""))
            hit = False
            for surface in surfaces:
                toks = _alias_tokens(surface)
                if toks and all(t in q_words for t in toks):
                    hit = True
                    break
            if not hit:
                continue
            docs = con.execute(
                "SELECT COUNT(DISTINCT document_id) FROM mentions WHERE entity_id = ?",
                (eid,),
            ).fetchone()
            n_docs = int(docs[0] or 0) if docs else 0
            if n_docs <= 0 or n_docs > doc_cap:
                continue
            matched.append(str(eid))
        if not matched:
            return []
        chunk_ids: list[str] = []
        seen: set[str] = set()
        for eid in matched:
            for (cid,) in con.execute(
                """
                SELECT DISTINCT chunk_id FROM mentions
                WHERE entity_id = ?
                ORDER BY chunk_id
                """,
                (eid,),
            ).fetchall():
                s = str(cid)
                if s in seen:
                    continue
                seen.add(s)
                chunk_ids.append(s)
        return chunk_ids
    except Exception:
        return []
    finally:
        con.close()


def prepare_chat_retrieval(
    sqlite_path: str,
    query: str,
    embed_model: str,
    top_k: int = _VECTOR_K,
    history: list[dict[str, Any]] | None = None,
    scope_ids: list[str] | None = None,
    app_data: str | None = None,
    skip_rerank: bool = False,
) -> dict[str, Any]:
    """Vector + keyword RRF, profile scope, label boost, optional rerank + second pass."""
    t0 = time.perf_counter()
    hist_raw = list(history or [])
    normalised_q = normalize_search_text(query or "")
    search_text = normalize_search_text(embed_query_text(normalised_q, hist_raw))
    explicit = [str(x) for x in (scope_ids or []) if x]
    scope_info = resolve_scope(sqlite_path, normalised_q, explicit_ids=explicit or None)
    profile_matches = list(scope_info.get("profileMatches") or [])
    scope_chosen = bool(scope_info.get("scopeChosen"))
    scope_title = scope_info.get("scopeTitle")
    effective_scope: list[str] = list(scope_info.get("scopeIds") or [])
    scope_reason = str(scope_info.get("scopeReason") or "")
    scope_filter: list[str] | None = effective_scope or None
    labels = question_label_mentions(normalised_q)

    try:
        pool, vector_n, keyword_n, label_n, entity_chunk_ids = _retrieve_pool(
            sqlite_path,
            embed_model,
            search_text,
            top_k,
            scope_filter,
            labels,
            profile_matches,
            inject_label_from_profiles=not bool(explicit),
        )
    except EmbeddingDimMismatch:
        trace = _build_trace(
            normalised_question=normalised_q,
            profile_matches=profile_matches,
            scope_ids=effective_scope,
            scope_reason=scope_reason,
            vector_n=0,
            keyword_n=0,
            label_n=0,
        )
        _log_trace(trace)
        return _empty_result(
            scope_ids=effective_scope,
            scope_title=scope_title,
            scope_chosen=scope_chosen,
            query_embedding_failed=True,
            profile_matches=profile_matches,
            trace=trace,
            search_text=search_text,
        )

    if not pool:
        trace = _build_trace(
            normalised_question=normalised_q,
            profile_matches=profile_matches,
            scope_ids=effective_scope,
            scope_reason=scope_reason,
            vector_n=vector_n,
            keyword_n=keyword_n,
            label_n=label_n,
        )
        _log_trace(trace)
        failed = _indexed_chunk_count(sqlite_path) > 0
        return _empty_result(
            scope_ids=effective_scope,
            scope_title=scope_title,
            scope_chosen=scope_chosen,
            query_embedding_failed=failed,
            profile_matches=profile_matches,
            suggestions=_suggestions_from_profiles(profile_matches) if not explicit else [],
            trace=trace,
            search_text=search_text,
        )

    retrieval_ms = int((time.perf_counter() - t0) * 1000)
    candidate_cites = [_citation_from_chunk(c, i + 1) for i, c in enumerate(pool)]

    if skip_rerank:
        keep = pool[:_KEEP]
        trace = _build_trace(
            normalised_question=normalised_q,
            profile_matches=profile_matches,
            scope_ids=effective_scope,
            scope_reason=scope_reason,
            vector_n=vector_n,
            keyword_n=keyword_n,
            label_n=label_n,
        )
        _log_trace(trace)
        return {
            "retrievalMode": "vector_only",
            "citations": [_citation_from_chunk(c, i + 1) for i, c in enumerate(keep)],
            "candidates": candidate_cites,
            "notFound": False,
            "bestScore": 0.0,
            "scopeIds": list(effective_scope),
            "scopeTitle": scope_title,
            "scopeChosen": scope_chosen,
            "scopeReason": scope_reason,
            "profileMatches": profile_matches,
            "suggestions": [],
            "entityChunks": list(entity_chunk_ids),
            "retrievalMs": retrieval_ms,
            "rerankMs": 0,
            "rerankScored": 0,
            "searchText": search_text,
            "trace": trace,
            "labels": labels,
            "normalisedQuestion": normalised_q,
            "candidateCounts": {
                "vector": vector_n,
                "keyword": keyword_n,
                "label": label_n,
            },
        }

    ranked, rr = _rerank_pool(app_data, search_text, pool)
    best_score = float(ranked[0][1]) if ranked else 0.0
    log.info(
        "retrieval %sms rerank %sms scored %s/%s",
        retrieval_ms,
        rr["rerankMs"],
        rr["scored"],
        len(pool),
    )

    second_pass = False
    suggestions: list[dict[str, Any]] = []
    if best_score < NOT_FOUND_SCORE and not explicit:
        for m in profile_matches[:2]:
            did = str(m["documentId"])
            try:
                pool2, _vn, _kn, _ln, _ = _retrieve_pool(
                    sqlite_path,
                    embed_model,
                    search_text,
                    top_k,
                    [did],
                    labels,
                    profile_matches,
                    inject_label_from_profiles=False,
                )
            except EmbeddingDimMismatch:
                continue
            if not pool2:
                continue
            ranked2, rr2 = _rerank_pool(app_data, search_text, pool2)
            rr = {
                "rerankMs": int(rr.get("rerankMs") or 0) + int(rr2.get("rerankMs") or 0),
                "scored": int(rr.get("scored") or 0) + int(rr2.get("scored") or 0),
            }
            best2 = float(ranked2[0][1]) if ranked2 else 0.0
            if best2 >= NOT_FOUND_SCORE:
                second_pass = True
                ranked = ranked2
                best_score = best2
                candidate_cites = [_citation_from_chunk(c, i + 1) for i, c in enumerate(pool2)]
                effective_scope = [did]
                scope_title = m.get("title") or None
                scope_chosen = True
                scope_reason = "second_pass"
                break
        else:
            suggestions = _suggestions_from_profiles(profile_matches)

    trace = _build_trace(
        normalised_question=normalised_q,
        profile_matches=profile_matches,
        scope_ids=effective_scope,
        scope_reason=scope_reason,
        vector_n=vector_n,
        keyword_n=keyword_n,
        label_n=label_n,
        reranked=ranked,
        second_pass=second_pass,
    )
    _log_trace(trace)

    if best_score < NOT_FOUND_SCORE:
        keep = [c for c, _s in ranked[:3]]
        return {
            "retrievalMode": "vector_only",
            "citations": [_citation_from_chunk(c, i + 1) for i, c in enumerate(keep)],
            "candidates": candidate_cites,
            "notFound": True,
            "bestScore": best_score,
            "scopeIds": list(effective_scope),
            "scopeTitle": scope_title,
            "scopeChosen": scope_chosen,
            "scopeReason": scope_reason,
            "profileMatches": profile_matches,
            "suggestions": suggestions
            if suggestions
            else (_suggestions_from_profiles(profile_matches) if not explicit else []),
            "entityChunks": list(entity_chunk_ids),
            "retrievalMs": retrieval_ms,
            "rerankMs": rr["rerankMs"],
            "rerankScored": rr["scored"],
            "searchText": search_text,
            "trace": trace,
        }

    keep = [c for c, _s in ranked[:_KEEP]]
    return {
        "retrievalMode": "vector_only",
        "citations": [_citation_from_chunk(c, i + 1) for i, c in enumerate(keep)],
        "candidates": candidate_cites,
        "notFound": False,
        "bestScore": best_score,
        "scopeIds": list(effective_scope),
        "scopeTitle": scope_title,
        "scopeChosen": scope_chosen,
        "scopeReason": scope_reason,
        "profileMatches": profile_matches,
        "suggestions": [],
        "entityChunks": list(entity_chunk_ids),
        "retrievalMs": retrieval_ms,
        "rerankMs": rr["rerankMs"],
        "rerankScored": rr["scored"],
        "searchText": search_text,
        "trace": trace,
    }


def rerank_chat_candidates(
    app_data: str,
    query: str,
    candidates: list[dict[str, Any]],
    *,
    sqlite_path: str | None = None,
    embed_model: str | None = None,
    profile_matches: list[dict[str, Any]] | None = None,
    explicit_scope: bool = False,
    labels: list[str] | None = None,
    normalised_question: str | None = None,
    scope_ids: list[str] | None = None,
    scope_reason: str | None = None,
    candidate_counts: dict[str, int] | None = None,
) -> dict[str, Any]:
    """Rerank fused candidates; optional scoped second pass before notFound."""
    search_text = normalize_search_text(query or "")
    matches = list(profile_matches or [])
    counts = candidate_counts or {}
    if not candidates:
        trace = _build_trace(
            normalised_question=normalised_question or search_text,
            profile_matches=matches,
            scope_ids=list(scope_ids or []),
            scope_reason=scope_reason or "empty_candidates",
            vector_n=int(counts.get("vector") or 0),
            keyword_n=int(counts.get("keyword") or 0),
            label_n=int(counts.get("label") or 0),
            second_pass=False,
        )
        _log_trace(trace)
        return {
            "citations": [],
            "notFound": True,
            "bestScore": 0.0,
            "rerankMs": 0,
            "rerankScored": 0,
            "suggestions": _suggestions_from_profiles(matches) if not explicit_scope else [],
            "scopeIds": list(scope_ids or []),
            "scopeTitle": None,
            "scopeChosen": False,
            "trace": trace,
        }

    ranked, rr = _rerank_pool(app_data, search_text, candidates)
    best_score = float(ranked[0][1]) if ranked else 0.0
    log.info("rerank %sms scored %s/%s", rr["rerankMs"], rr["scored"], len(candidates))

    second_pass = False
    out_scope_ids = list(scope_ids or [])
    out_scope_title: str | None = None
    out_scope_chosen = False
    out_scope_reason = scope_reason or ""
    suggestions: list[dict[str, Any]] = []
    label_list = list(labels or []) or question_label_mentions(normalised_question or search_text)

    if (
        best_score < NOT_FOUND_SCORE
        and not explicit_scope
        and sqlite_path
        and embed_model
        and matches
    ):
        for m in matches[:2]:
            did = str(m["documentId"])
            try:
                pool2, _vn, _kn, _ln, _ = _retrieve_pool(
                    sqlite_path,
                    embed_model,
                    search_text,
                    _VECTOR_K,
                    [did],
                    label_list,
                    matches,
                    inject_label_from_profiles=False,
                )
            except EmbeddingDimMismatch:
                continue
            if not pool2:
                continue
            ranked2, rr2 = _rerank_pool(app_data, search_text, pool2)
            rr = {
                "rerankMs": int(rr.get("rerankMs") or 0) + int(rr2.get("rerankMs") or 0),
                "scored": int(rr.get("scored") or 0) + int(rr2.get("scored") or 0),
            }
            best2 = float(ranked2[0][1]) if ranked2 else 0.0
            if best2 >= NOT_FOUND_SCORE:
                second_pass = True
                ranked = [(_citation_from_chunk(c, 0), s) for c, s in ranked2]
                best_score = best2
                out_scope_ids = [did]
                out_scope_title = m.get("title") or None
                out_scope_chosen = True
                out_scope_reason = "second_pass"
                break
        else:
            suggestions = _suggestions_from_profiles(matches)

    matched_doc_ids = {str(m["documentId"]) for m in matches if m.get("documentId")}
    matched_doc_ids.update(str(x) for x in out_scope_ids if x)
    ranked, label_covers = _pin_label_chunks(
        ranked,
        label_list,
        sqlite_path=sqlite_path,
        matched_doc_ids=matched_doc_ids,
    )

    found = best_score >= NOT_FOUND_SCORE or label_covers

    trace = _build_trace(
        normalised_question=normalised_question or search_text,
        profile_matches=matches,
        scope_ids=out_scope_ids,
        scope_reason=out_scope_reason or ("not_found" if not found else "rerank"),
        vector_n=int(counts.get("vector") or 0),
        keyword_n=int(counts.get("keyword") or 0),
        label_n=int(counts.get("label") or 0),
        reranked=[
            (
                {
                    "chunk_id": c.get("chunkId") or c.get("chunk_id"),
                    "document_id": c.get("documentId") or c.get("document_id"),
                    "title": c.get("title"),
                    "page_start": c.get("pageStart") or c.get("page_start"),
                },
                s,
            )
            for c, s in ranked
        ],
        second_pass=second_pass,
    )
    _log_trace(trace)

    def _cite_row(c: dict[str, Any], index: int) -> dict[str, Any]:
        if c.get("chunkId") is not None and "snippet" in c:
            item = dict(c)
            item["index"] = index
            if not item.get("text"):
                item["text"] = item.get("snippet") or ""
            return item
        return _citation_from_chunk(
            {
                "document_id": c.get("documentId") or c.get("document_id"),
                "title": c.get("title") or "",
                "path": c.get("path") or "",
                "page_start": c.get("pageStart") or c.get("page_start"),
                "page_end": c.get("pageEnd") or c.get("page_end"),
                "chunk_id": c.get("chunkId") or c.get("chunk_id"),
                "text": c.get("text") or c.get("snippet") or "",
                "regions": c.get("regions"),
                "labels": c.get("labels"),
                "ordinal": c.get("ordinal"),
            },
            index,
        )

    if not found:
        cites = [_cite_row(c, i + 1) for i, (c, _s) in enumerate(ranked[:3])]
        return {
            "citations": cites,
            "notFound": True,
            "bestScore": best_score,
            "rerankMs": rr["rerankMs"],
            "rerankScored": rr["scored"],
            "suggestions": suggestions
            if suggestions
            else (_suggestions_from_profiles(matches) if not explicit_scope else []),
            "scopeIds": out_scope_ids,
            "scopeTitle": out_scope_title,
            "scopeChosen": out_scope_chosen,
            "trace": trace,
        }

    cites = [_cite_row(c, i + 1) for i, (c, _s) in enumerate(ranked[:_KEEP])]
    return {
        "citations": cites,
        "notFound": False,
        "bestScore": best_score,
        "rerankMs": rr["rerankMs"],
        "rerankScored": rr["scored"],
        "suggestions": [],
        "scopeIds": out_scope_ids,
        "scopeTitle": out_scope_title,
        "scopeChosen": out_scope_chosen,
        "trace": trace,
    }


def prepare_related_retrieval(
    sqlite_path: str,
    entity_id_a: str,
    entity_id_b: str,
    embed_model: str,
    app_data: str | None = None,
    top_k: int = _KEEP,
) -> dict[str, Any]:
    """Passages where both entities are mentioned, plus best for each alone."""
    del embed_model
    empty: dict[str, Any] = {
        "citations": [],
        "bothChunkIds": [],
        "aChunkIds": [],
        "bChunkIds": [],
    }
    both: list[str] = []
    only_a: list[str] = []
    only_b: list[str] = []
    a_list: list[str] = []
    b_list: list[str] = []
    try:
        con = connect_sqlite(sqlite_path)
    except Exception:
        return empty
    try:
        if not _table_exists(con, "mentions"):
            return empty

        def chunk_ids_for(eid: str) -> list[str]:
            rows = con.execute(
                """
                SELECT DISTINCT chunk_id FROM mentions
                WHERE entity_id = ?
                ORDER BY chunk_id
                """,
                (eid,),
            ).fetchall()
            return [str(r[0]) for r in rows]

        a_ids = set(chunk_ids_for(entity_id_a))
        b_ids = set(chunk_ids_for(entity_id_b))
        both = sorted(a_ids & b_ids)
        only_a = sorted(a_ids - b_ids)
        only_b = sorted(b_ids - a_ids)
        a_list = sorted(a_ids)
        b_list = sorted(b_ids)
    finally:
        con.close()

    ordered_ids = list(both) + list(only_a[:3]) + list(only_b[:3])
    chunks = _chunks_by_ids(sqlite_path, ordered_ids)
    if app_data and chunks:
        passages = [str(c.get("text") or "") for c in chunks]
        q = f"{entity_id_a} {entity_id_b}"
        rr = rerank_with_budget(app_data, q, passages)
        ranked = sorted(
            zip(chunks, rr["scores"]),
            key=lambda pair: (-pair[1], str(pair[0].get("chunk_id") or "")),
        )
        keep = [c for c, _s in ranked[:top_k]]
    else:
        keep = chunks[:top_k]
    return {
        "citations": [_citation_from_chunk(c, i + 1) for i, c in enumerate(keep)],
        "bothChunkIds": both,
        "aChunkIds": a_list,
        "bChunkIds": b_list,
    }
