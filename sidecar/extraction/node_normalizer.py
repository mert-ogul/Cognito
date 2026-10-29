"""Normalize entity names for deduplication."""

from __future__ import annotations

import re
import unicodedata

_TRAILING_PUNCT = re.compile(r"[\s\"'`.,;:!?)\]]+$")
_LEADING_PUNCT = re.compile(r"^[\[(\"'`]+")


def normalize_name(name: str) -> str:
    s = unicodedata.normalize("NFKC", name or "")
    s = s.strip().lower()
    s = _LEADING_PUNCT.sub("", s)
    s = _TRAILING_PUNCT.sub("", s)
    s = s.strip()
    if s.startswith("the "):
        s = s[4:].strip()
    s = re.sub(r"\s+", " ", s)
    return s
