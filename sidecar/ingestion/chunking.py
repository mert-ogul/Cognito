"""Semantic-ish token-targeted chunking (~512 tokens, ~50 overlap)."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass

TARGET = 512
OVERLAP = 50


@dataclass
class ChunkDraft:
    text: str
    page_start: int | None
    page_end: int | None


def estimate_tokens(text: str) -> int:
    return max(1, len(text) // 4)


def hash_chunk_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def chunk_semantic(text: str, _file_type: str) -> list[ChunkDraft]:
    out: list[ChunkDraft] = []
    t = text.strip()
    if not t:
        return out
    i = 0
    n = len(t)
    overlap_chars = max(24, OVERLAP * 4)
    while i < n:
        j = i
        tokens = 0
        while j < n and tokens < TARGET:
            step = min(120, n - j)
            tokens += estimate_tokens(t[j : j + step])
            j += step
        piece = t[i:j].strip()
        if piece:
            out.append(ChunkDraft(text=piece, page_start=None, page_end=None))
        if j >= n:
            break
        i = max(i + 1, j - overlap_chars)
    return out


def chunk_text(text: str, max_chars: int = 1800, overlap: int = 200) -> list[ChunkDraft]:
    """Legacy char-based chunking for tests compatibility."""
    chunks: list[ChunkDraft] = []
    if not text.strip():
        return chunks
    pos = 0
    ln = len(text)
    while pos < ln:
        end = min(ln, pos + max_chars)
        piece = text[pos:end].strip()
        if piece:
            chunks.append(ChunkDraft(text=piece, page_start=None, page_end=None))
        if end >= ln:
            break
        pos = end - overlap
    return chunks
