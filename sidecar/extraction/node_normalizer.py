"""Normalize entity names for deduplication."""

from __future__ import annotations

import re
import unicodedata


def normalize_name(name: str) -> str:
    s = unicodedata.normalize("NFKC", name or "")
    s = s.strip().lower()
    s = re.sub(r"\s+", " ", s)
    return s
