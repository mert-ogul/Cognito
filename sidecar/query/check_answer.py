"""Ground draft answers against retrieved passages (quotes + HHEM)."""

from __future__ import annotations

import re
import unicodedata
from typing import Any

from common.model_store import hhem_scores_bounded

_HHEM_MIN = 0.5
_ASCII_LETTER_RATIO = 0.6
_PASSAGE_CAP = 2000
_QUOTE_SKIPS = 2
_WORD_RE = re.compile(r"[^\W_]+")

_LIGATURES = str.maketrans(
    {
        "ﬁ": "fi",
        "ﬂ": "fl",
        "ﬀ": "ff",
        "ﬃ": "ffi",
        "ﬄ": "ffl",
        "æ": "ae",
        "œ": "oe",
        "Æ": "AE",
        "Œ": "OE",
    }
)
_QUOTE_MARKS = str.maketrans(
    {
        "“": '"',
        "”": '"',
        "„": '"',
        "«": '"',
        "»": '"',
        "‘": "'",
        "’": "'",
        "‚": "'",
        "′": "'",
        "″": '"',
    }
)
_HYPHEN_BREAK_RE = re.compile(r"(\w)-\s*\n\s*(\w)")
_WS_RE = re.compile(r"\s+")
_NUMBER_RE = re.compile(
    r"\b(?:\d{1,4}[-/]\d{1,2}[-/]\d{1,4}|\d+(?:\.\d+)?(?:st|nd|rd|th)?)\b",
    re.IGNORECASE,
)
_MULTI_NAME_RE = re.compile(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b")


def normalize_evidence_text(text: str) -> str:
    s = (text or "").translate(_LIGATURES).translate(_QUOTE_MARKS)
    s = _HYPHEN_BREAK_RE.sub(r"\1\2", s)
    s = _WS_RE.sub(" ", s).strip()
    return s


def evidence_words(text: str) -> list[str]:
    s = unicodedata.normalize("NFKD", normalize_evidence_text(text))
    s = "".join(ch for ch in s if unicodedata.category(ch) != "Mn")
    return _WORD_RE.findall(s.lower())


def quote_in_passage(quote: str, passage: str) -> bool:
    q = evidence_words(quote)
    p = evidence_words(passage)
    if not q:
        return False
    for start, word in enumerate(p):
        if word != q[0]:
            continue
        j, skips, matched = start + 1, 0, True
        for w in q[1:]:
            while j < len(p) and p[j] != w and skips < _QUOTE_SKIPS:
                j += 1
                skips += 1
            if j < len(p) and p[j] == w:
                j += 1
            else:
                matched = False
                break
        if matched:
            return True
    return False


def looks_like_formula(text: str) -> bool:
    if "$" in text or "\\" in text:
        return True
    words = evidence_words(text)
    if not words:
        return True
    plain = sum(1 for w in words if len(w) >= 3 and w.isalpha())
    return plain * 2 < len(words)


def _ascii_letter_ratio(text: str) -> float:
    letters = [c for c in text if c.isalpha()]
    if not letters:
        return 0.0
    ascii_letters = sum(1 for c in letters if ord(c) < 128)
    return ascii_letters / len(letters)


def _mostly_english(a: str, b: str) -> bool:
    return (
        _ascii_letter_ratio(a) >= _ASCII_LETTER_RATIO
        and _ascii_letter_ratio(b) >= _ASCII_LETTER_RATIO
    )


def _passage_by_index(passages: list[dict[str, Any]]) -> dict[int, str]:
    out: dict[int, str] = {}
    for p in passages:
        try:
            idx = int(p["index"])
        except (KeyError, TypeError, ValueError):
            continue
        out[idx] = str(p.get("text") or "")
    return out


def _claims_in_sentence(sentence: str) -> list[str]:
    claims: list[str] = []
    for m in _NUMBER_RE.finditer(sentence or ""):
        claims.append(m.group(0))
    for m in _MULTI_NAME_RE.finditer(sentence or ""):
        claims.append(m.group(1))
    return claims


def _claim_supported(claim: str, haystacks: list[str]) -> bool:
    needle = normalize_evidence_text(claim).lower()
    if not needle:
        return True
    for h in haystacks:
        if needle in normalize_evidence_text(h).lower():
            return True
    return False


def check_answer(app_data: str, passages: list[dict], draft: dict) -> dict:
    answerable = bool(draft.get("answerable"))
    answer = draft.get("answer") or []
    if not answer:
        return {"notFound": True, "sentences": []}

    by_idx = _passage_by_index(passages)
    raw_quotes = draft.get("quotes") or []
    valid_quotes: list[dict[str, Any] | None] = []
    for q in raw_quotes:
        try:
            source = int(q.get("source"))
        except (TypeError, ValueError, AttributeError):
            valid_quotes.append(None)
            continue
        qtext = str(q.get("text") or "")
        passage = by_idx.get(source)
        if passage is None:
            valid_quotes.append(None)
            continue
        if quote_in_passage(qtext, passage):
            valid_quotes.append({"source": source, "text": qtext})
            continue
        if looks_like_formula(qtext):
            valid_quotes.append({"source": source, "text": qtext, "formula": True})
            continue
        valid_quotes.append(None)

    sentences_out: list[dict[str, Any]] = []
    any_ok = False
    hhem_jobs: list[tuple[int, str, str]] = []

    for sent in answer:
        text = str(sent.get("sentence") or sent.get("text") or "")
        q_indexes = sent.get("quotes") or []
        kept: list[dict[str, Any]] = []
        for qi in q_indexes:
            try:
                i = int(qi)
            except (TypeError, ValueError):
                continue
            if 0 <= i < len(valid_quotes) and valid_quotes[i] is not None:
                kept.append(valid_quotes[i])  # type: ignore[arg-type]
        sources = sorted({int(q["source"]) for q in kept})
        quote_texts = [str(q["text"]) for q in kept]
        passage_texts = [by_idx[s] for s in sources if s in by_idx]
        haystacks = quote_texts + passage_texts

        formula_sentence = looks_like_formula(text)
        formula_only = bool(kept) and all(bool(q.get("formula")) for q in kept)
        if formula_sentence or formula_only:
            sentences_out.append(
                {
                    "text": text,
                    "status": "unchecked",
                    "sources": sources,
                    "reason": "formula",
                }
            )
            any_ok = True
            continue

        ok_quotes = len(kept) >= 1
        claims = _claims_in_sentence(text)
        ok_claims = all(_claim_supported(c, haystacks) for c in claims)
        status = "unsupported"
        reason: str | None = None
        if ok_quotes and ok_claims:
            premise = " ".join(t[:_PASSAGE_CAP] for t in passage_texts)
            if _mostly_english(premise, text):
                hhem_jobs.append((len(sentences_out), premise, text))
                status = "pending"
            else:
                status = "unchecked"
                reason = "language"
                any_ok = True
        row: dict[str, Any] = {"text": text, "status": status, "sources": sources}
        if reason:
            row["reason"] = reason
        sentences_out.append(row)

    if hhem_jobs:
        scores = hhem_scores_bounded(app_data, [(j, s) for _i, j, s in hhem_jobs])
        for (idx, _j, _s), score in zip(hhem_jobs, scores):
            if score is None:
                sentences_out[idx]["status"] = "unchecked"
                sentences_out[idx]["reason"] = "budget"
                any_ok = True
            elif float(score) >= _HHEM_MIN:
                sentences_out[idx]["status"] = "supported"
                any_ok = True
            else:
                sentences_out[idx]["status"] = "unsupported"

    for s in sentences_out:
        if s["status"] == "pending":
            s["status"] = "unchecked"
            s["reason"] = "budget"
            any_ok = True

    if not answerable:
        return {"notFound": True, "sentences": sentences_out}

    return {"notFound": not any_ok, "sentences": sentences_out}
