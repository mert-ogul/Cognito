"""Natural-boundary chunking (~512 tokens, ~50 overlap)."""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field

TARGET = 512
OVERLAP = 50

_HEADING_RE = re.compile(r"^(#{1,6})\s+(.*)$")
_SENTENCE_RE = re.compile(r"(?<=[.!?])\s+")


@dataclass
class ChunkDraft:
    text: str
    page_start: int | None
    page_end: int | None


@dataclass
class _Block:
    text: str
    page: int | None = None
    heading_trail: list[str] = field(default_factory=list)


def estimate_tokens(text: str) -> int:
    return max(1, len(text) // 4)


def hash_chunk_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _update_heading_trail(trail: list[str], line: str) -> list[str]:
    m = _HEADING_RE.match(line.strip())
    if not m:
        return trail
    level = len(m.group(1))
    title = m.group(2).strip()
    return trail[: level - 1] + [title]


def _paragraphs_from_text(text: str) -> list[str]:
    parts = re.split(r"\n\s*\n", text)
    out = [p.strip() for p in parts if p.strip()]
    if out:
        return out
    return [ln.strip() for ln in text.splitlines() if ln.strip()]


def _markdown_blocks(text: str) -> list[_Block]:
    lines = text.splitlines()
    blocks: list[_Block] = []
    trail: list[str] = []
    buf: list[str] = []
    in_fence = False

    def flush() -> None:
        nonlocal buf
        body = "\n".join(buf).strip()
        buf = []
        if body:
            blocks.append(_Block(text=body, page=None, heading_trail=list(trail)))

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("```"):
            if in_fence:
                buf.append(line)
                in_fence = False
                flush()
            else:
                flush()
                in_fence = True
                buf.append(line)
            continue
        if in_fence:
            buf.append(line)
            continue
        if _HEADING_RE.match(stripped):
            flush()
            trail = _update_heading_trail(trail, stripped)
            blocks.append(_Block(text=stripped, page=None, heading_trail=list(trail)))
            continue
        if not stripped:
            flush()
            continue
        buf.append(line)
    flush()
    return blocks


def _pdf_blocks(pages: list[dict] | None, text: str) -> list[_Block]:
    if pages:
        blocks: list[_Block] = []
        for p in pages:
            page_no = int(p.get("page", 0)) or None
            for para in _paragraphs_from_text(str(p.get("text", ""))):
                blocks.append(_Block(text=para, page=page_no))
        if blocks:
            return blocks
    return [_Block(text=p, page=None) for p in _paragraphs_from_text(text)]


def _split_at_whitespace(text: str, max_tokens: int) -> list[str]:
    """Split text so each piece is ≤ max_tokens, never mid-word."""
    text = text.strip()
    if not text:
        return []
    if estimate_tokens(text) <= max_tokens:
        return [text]
    out: list[str] = []
    remaining = text
    while remaining:
        if estimate_tokens(remaining) <= max_tokens:
            out.append(remaining.strip())
            break
        # Approximate character budget from token target.
        budget = max(4, max_tokens * 4)
        cut = min(len(remaining), budget)
        # Prefer last whitespace at or before cut; never mid-word.
        if cut < len(remaining) and not remaining[cut].isspace():
            ws = remaining.rfind(" ", 0, cut + 1)
            if ws > 0:
                cut = ws
            else:
                # No whitespace in window: advance to next whitespace or end.
                nxt = remaining.find(" ", cut)
                cut = len(remaining) if nxt < 0 else nxt
        piece = remaining[:cut].strip()
        if piece:
            out.append(piece)
        remaining = remaining[cut:].lstrip()
        if not piece and remaining:
            # Safety: consume one character to avoid infinite loop on odd input.
            out.append(remaining[0])
            remaining = remaining[1:].lstrip()
    return out


def _split_oversized_block(block: _Block) -> list[_Block]:
    if estimate_tokens(block.text) <= TARGET:
        return [block]
    pieces: list[_Block] = []
    sentences = _SENTENCE_RE.split(block.text.strip())
    if len(sentences) <= 1:
        for part in _split_at_whitespace(block.text, TARGET):
            pieces.append(
                _Block(text=part, page=block.page, heading_trail=list(block.heading_trail))
            )
        return pieces
    buf: list[str] = []
    for sent in sentences:
        sent = sent.strip()
        if not sent:
            continue
        candidate = (" ".join(buf + [sent])).strip()
        if buf and estimate_tokens(candidate) > TARGET:
            for part in _split_at_whitespace(" ".join(buf), TARGET):
                pieces.append(
                    _Block(text=part, page=block.page, heading_trail=list(block.heading_trail))
                )
            buf = [sent]
            if estimate_tokens(sent) > TARGET:
                for part in _split_at_whitespace(sent, TARGET):
                    pieces.append(
                        _Block(
                            text=part,
                            page=block.page,
                            heading_trail=list(block.heading_trail),
                        )
                    )
                buf = []
        else:
            buf.append(sent)
    if buf:
        for part in _split_at_whitespace(" ".join(buf), TARGET):
            pieces.append(
                _Block(text=part, page=block.page, heading_trail=list(block.heading_trail))
            )
    return pieces


def _expand_blocks(blocks: list[_Block]) -> list[_Block]:
    out: list[_Block] = []
    for b in blocks:
        out.extend(_split_oversized_block(b))
    return out


def _last_sentences_for_overlap(text: str, max_tokens: int) -> str:
    text = text.strip()
    if not text or max_tokens <= 0:
        return ""
    sentences = [s.strip() for s in _SENTENCE_RE.split(text) if s.strip()]
    if not sentences:
        return ""
    picked: list[str] = []
    for sent in reversed(sentences):
        candidate = " ".join([sent] + picked)
        if picked and estimate_tokens(candidate) > max_tokens:
            break
        picked.insert(0, sent)
        if estimate_tokens(" ".join(picked)) >= max_tokens:
            break
    return " ".join(picked).strip()


def _trail_line(trail: list[str]) -> str:
    return " > ".join(t for t in trail if t)


def _pack_blocks(blocks: list[_Block], file_type: str) -> list[ChunkDraft]:
    blocks = _expand_blocks(blocks)
    if not blocks:
        return []
    chunks: list[ChunkDraft] = []
    i = 0
    prev_body = ""
    while i < len(blocks):
        pack: list[_Block] = []
        tokens = 0
        while i < len(blocks):
            b = blocks[i]
            add = estimate_tokens(b.text)
            if pack and tokens + add > TARGET:
                only_headings = all(_HEADING_RE.match(x.text.strip()) for x in pack)
                if only_headings:
                    pack.append(b)
                    tokens += add
                    i += 1
                break
            pack.append(b)
            tokens += add
            i += 1
            if tokens >= TARGET:
                break
        if not pack:
            pack = [blocks[i]]
            i += 1
        body = "\n\n".join(b.text for b in pack).strip()
        trail = pack[0].heading_trail if file_type == "markdown" else []
        header = _trail_line(trail)
        parts: list[str] = []
        if header:
            first_line = body.splitlines()[0].strip() if body else ""
            hm = _HEADING_RE.match(first_line)
            already_has_heading = bool(hm and trail and hm.group(2).strip() == trail[-1])
            if not already_has_heading and not body.startswith(header):
                parts.append(header)
        if chunks and prev_body:
            overlap = _last_sentences_for_overlap(prev_body, OVERLAP)
            if overlap and overlap not in body[: len(overlap) + 8]:
                parts.append(overlap)
        parts.append(body)
        text = "\n\n".join(p for p in parts if p).strip()
        pages = [b.page for b in pack if b.page is not None]
        page_start = min(pages) if pages else None
        page_end = max(pages) if pages else None
        chunks.append(ChunkDraft(text=text, page_start=page_start, page_end=page_end))
        prev_body = body
    return chunks


def chunk_semantic(
    text: str,
    file_type: str,
    pages: list[dict] | None = None,
) -> list[ChunkDraft]:
    t = text.strip()
    if not t and not pages:
        return []
    ft = (file_type or "markdown").lower()
    if ft == "pdf":
        blocks = _pdf_blocks(pages, text)
    else:
        blocks = _markdown_blocks(text if t else "")
    return _pack_blocks(blocks, "markdown" if ft != "pdf" else "pdf")


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
