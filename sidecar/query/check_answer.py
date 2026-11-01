"""Ground draft answers against retrieved passages (quotes + HHEM)."""

from __future__ import annotations

import math
import re
import unicodedata
from typing import Any

from common.model_store import hhem_scores_bounded

_HHEM_MIN = 0.5
_ASCII_LETTER_RATIO = 0.6
_PASSAGE_CAP = 2000
_QUOTE_SKIPS = 2
_WORD_RE = re.compile(r"[^\W_]+")
_TOKEN_RE = re.compile(r"[^\W_]+(?:[-'][^\W_]+)*")

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
_LIST_MARKER_RE = re.compile(r"(?m)^\s*(?:[-*+]|\d+[.)])\s+")
_NUMBER_RE = re.compile(
    r"\b(?:\d{1,4}[-/]\d{1,2}[-/]\d{1,4}|\d+(?:\.\d+)?(?:st|nd|rd|th)?)\b",
    re.IGNORECASE,
)


def normalize_evidence_text(text: str) -> str:
    s = (text or "").translate(_LIGATURES).translate(_QUOTE_MARKS)
    s = _HYPHEN_BREAK_RE.sub(r"\1\2", s)
    s = _LIST_MARKER_RE.sub(" ", s)
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


_MATH_SPAN_RE = re.compile(r"\$\$[\s\S]*?\$\$|\$[^$]*\$")


def looks_like_formula(text: str) -> bool:
    s = text or ""
    without = _MATH_SPAN_RE.sub(" ", s)
    remaining = evidence_words(without)
    if sum(1 for w in remaining if len(w) >= 3 and w.isalpha()) >= 4:
        return False
    words = evidence_words(s)
    if not words:
        return True
    plain = sum(1 for w in words if len(w) >= 3 and w.isalpha())
    return plain * 2 < len(words)


def numbers_supported(sentence: str, evidence: list[str]) -> bool:
    numbers, _names = claims_in_sentence(sentence)
    text = " ".join(normalize_evidence_text(e).lower() for e in evidence)
    return all(normalize_evidence_text(n).lower() in text for n in numbers)


def claims_in_sentence(sentence: str) -> tuple[list[str], list[str]]:
    numbers = [m.group(0) for m in _NUMBER_RE.finditer(sentence)]
    names: list[str] = []
    for i, word in enumerate(_TOKEN_RE.findall(sentence)):
        if any(c.isdigit() for c in word):
            continue
        if (word.isupper() and len(word) >= 2) or (i > 0 and word[0].isupper()):
            names.append(word)
    return numbers, names


def claims_supported(sentence: str, evidence: list[str]) -> bool:
    numbers, names = claims_in_sentence(sentence)
    text = " ".join(normalize_evidence_text(e).lower() for e in evidence)
    words = set(evidence_words(" ".join(evidence)))
    if any(normalize_evidence_text(n).lower() not in text for n in numbers):
        return False
    return all(all(w in words for w in evidence_words(n)) for n in names)


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


def _passage_evidence(p: dict[str, Any]) -> str:
    title = str(p.get("title") or "").strip()
    text = str(p.get("text") or "")
    if title:
        return f"{title}\n{text}"
    return text


def _passage_by_index(passages: list[dict[str, Any]]) -> dict[int, str]:
    out: dict[int, str] = {}
    for p in passages:
        try:
            idx = int(p["index"])
        except (KeyError, TypeError, ValueError):
            continue
        out[idx] = _passage_evidence(p)
    return out


def _resolve_quote(qtext: str, source: int, by_idx: dict[int, str]) -> dict[str, Any] | None:
    passage = by_idx.get(source)
    if passage is not None and quote_in_passage(qtext, passage):
        return {"source": source, "text": qtext}
    if looks_like_formula(qtext):
        return {"source": source, "text": qtext, "formula": True}
    for other_idx, other_text in by_idx.items():
        if other_idx == source:
            continue
        if quote_in_passage(qtext, other_text):
            return {"source": other_idx, "text": qtext}
    return None


def check_answer(app_data: str, passages: list[dict], draft: dict) -> dict:
    answerable = bool(draft.get("answerable"))
    if not answerable:
        return {"notFound": True, "sentences": []}
    answer = draft.get("answer") or []
    if not answer:
        return {"notFound": True, "sentences": []}

    by_idx = _passage_by_index(passages)
    sentences_out: list[dict[str, Any]] = []
    hhem_jobs: list[tuple[int, str, str]] = []

    for sent in answer:
        text = str(sent.get("sentence") or sent.get("text") or "")
        kept: list[dict[str, Any]] = []
        for q in sent.get("quotes") or []:
            if not isinstance(q, dict):
                continue
            try:
                source = int(q.get("source"))
            except (TypeError, ValueError, AttributeError):
                continue
            resolved = _resolve_quote(str(q.get("text") or ""), source, by_idx)
            if resolved is not None:
                kept.append(resolved)
        sources = sorted({int(q["source"]) for q in kept})
        quote_texts = [str(q["text"]) for q in kept]
        passage_texts = [by_idx[s] for s in sources if s in by_idx]
        haystacks = quote_texts + passage_texts

        if looks_like_formula(text):
            if len(kept) >= 1 and numbers_supported(text, haystacks):
                sentences_out.append(
                    {
                        "text": text,
                        "status": "unchecked",
                        "sources": sources,
                        "reason": "formula",
                    }
                )
            else:
                sentences_out.append({"text": text, "status": "unsupported", "sources": sources})
            continue

        if len(kept) >= 1 and claims_supported(text, haystacks):
            status = "supported"
            reason: str | None = None
            premise = " ".join(t[:_PASSAGE_CAP] for t in passage_texts)
            if _mostly_english(premise, text):
                hhem_jobs.append((len(sentences_out), premise, text))
            row: dict[str, Any] = {"text": text, "status": status, "sources": sources}
            if reason:
                row["reason"] = reason
            sentences_out.append(row)
        else:
            sentences_out.append({"text": text, "status": "unsupported", "sources": sources})

    if hhem_jobs:
        scores = hhem_scores_bounded(app_data, [(j, s) for _i, j, s in hhem_jobs])
        for (idx, _j, _s), score in zip(hhem_jobs, scores):
            if score is None:
                sentences_out[idx]["status"] = "unchecked"
                sentences_out[idx]["reason"] = "budget"
            elif isinstance(score, float) and math.isnan(score):
                sentences_out[idx]["status"] = "unchecked"
                sentences_out[idx]["reason"] = "error"
            elif float(score) < _HHEM_MIN:
                sentences_out[idx]["status"] = "unchecked"
                sentences_out[idx]["reason"] = "weak"

    any_shown = any(s["status"] in ("supported", "unchecked") for s in sentences_out)
    return {"notFound": not any_shown, "sentences": sentences_out}
