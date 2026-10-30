"""Natural-boundary chunking (~512 tokens, ~50 overlap)."""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field

TARGET = 512
OVERLAP = 50
_REGION_GAP = 4.0

_HEADING_RE = re.compile(r"^(#{1,6})\s+(.*)$")
_SENTENCE_RE = re.compile(r"(?<=[.!?])\s+")
_REF_SECTION_KEYS = frozenset(
    {"references", "bibliography", "acknowledgments", "acknowledgements"}
)
_CITATION_LINE_RE = re.compile(
    r"^\s*(?:\[\d+\]|\d+\.)\s+.*\b(?:19|20)\d{2}\b",
    re.IGNORECASE,
)
_EQ_LABEL_RE = re.compile(r"^\s*(\(\d+\))\s*$")
_CAPTION_LABEL_RE = re.compile(
    r"^\s*((?:Table|Figure|Fig\.)\s+\d+)\b",
    re.IGNORECASE,
)


@dataclass
class ChunkDraft:
    text: str
    page_start: int | None
    page_end: int | None
    heading_trail: list[str] = field(default_factory=list)
    role: str = "body"
    regions: list[dict] | None = None


@dataclass
class _Block:
    text: str
    page: int | None = None
    heading_trail: list[str] = field(default_factory=list)
    bbox: tuple[float, float, float, float] | None = None


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


def _bbox_from_raw(raw: object) -> tuple[float, float, float, float] | None:
    if not isinstance(raw, (list, tuple)) or len(raw) < 4:
        return None
    try:
        return (float(raw[0]), float(raw[1]), float(raw[2]), float(raw[3]))
    except (TypeError, ValueError):
        return None


def _pdf_blocks(pages: list[dict] | None, text: str) -> list[_Block]:
    if pages:
        blocks: list[_Block] = []
        for p in pages:
            page_no = int(p.get("page", 0)) or None
            raw_blocks = p.get("blocks")
            if isinstance(raw_blocks, list) and raw_blocks:
                for rb in raw_blocks:
                    if not isinstance(rb, dict):
                        continue
                    bt = str(rb.get("text") or "").strip()
                    if not bt:
                        continue
                    bbox = _bbox_from_raw(rb.get("bbox"))
                    blocks.append(_Block(text=bt, page=page_no, bbox=bbox))
                continue
            for para in _paragraphs_from_text(str(p.get("text", ""))):
                blocks.append(_Block(text=para, page=page_no))
        if blocks:
            return blocks
    return [_Block(text=p, page=None) for p in _paragraphs_from_text(text)]


def _rects_overlap_or_near(
    a: tuple[float, float, float, float],
    b: tuple[float, float, float, float],
    gap: float,
) -> bool:
    ax0, ay0, ax1, ay1 = a
    bx0, by0, bx1, by1 = b
    if ax1 + gap < bx0 or bx1 + gap < ax0:
        return False
    if ay1 + gap < by0 or by1 + gap < ay0:
        return False
    return True


def _union_rect(
    a: tuple[float, float, float, float],
    b: tuple[float, float, float, float],
) -> tuple[float, float, float, float]:
    return (min(a[0], b[0]), min(a[1], b[1]), max(a[2], b[2]), max(a[3], b[3]))


def merge_regions(
    regions: list[dict],
    *,
    gap: float = _REGION_GAP,
) -> list[dict]:
    """Union overlapping/adjacent rects on the same page."""
    by_page: dict[int, list[tuple[float, float, float, float]]] = {}
    for r in regions:
        try:
            page = int(r["page"])
            box = (float(r["x0"]), float(r["y0"]), float(r["x1"]), float(r["y1"]))
        except (KeyError, TypeError, ValueError):
            continue
        by_page.setdefault(page, []).append(box)

    out: list[dict] = []
    for page in sorted(by_page):
        boxes = by_page[page]
        merged: list[tuple[float, float, float, float]] = []
        for box in sorted(boxes, key=lambda b: (b[1], b[0])):
            placed = False
            for i, cur in enumerate(merged):
                if _rects_overlap_or_near(cur, box, gap):
                    merged[i] = _union_rect(cur, box)
                    placed = True
                    break
            if not placed:
                merged.append(box)
        # Second pass: unions can create new overlaps after sequential merges.
        changed = True
        while changed:
            changed = False
            nxt: list[tuple[float, float, float, float]] = []
            for box in merged:
                absorbed = False
                for i, cur in enumerate(nxt):
                    if _rects_overlap_or_near(cur, box, gap):
                        nxt[i] = _union_rect(cur, box)
                        absorbed = True
                        changed = True
                        break
                if not absorbed:
                    nxt.append(box)
            merged = nxt
        for x0, y0, x1, y1 in merged:
            out.append({"page": page, "x0": x0, "y0": y0, "x1": x1, "y1": y1})
    return out


def _regions_from_pack(pack: list[_Block]) -> list[dict] | None:
    raw: list[dict] = []
    for b in pack:
        if b.page is None or b.bbox is None:
            continue
        x0, y0, x1, y1 = b.bbox
        raw.append({"page": b.page, "x0": x0, "y0": y0, "x1": x1, "y1": y1})
    if not raw:
        return None
    return merge_regions(raw)


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
                _Block(
                    text=part,
                    page=block.page,
                    heading_trail=list(block.heading_trail),
                    bbox=block.bbox,
                )
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
                    _Block(
                        text=part,
                        page=block.page,
                        heading_trail=list(block.heading_trail),
                        bbox=block.bbox,
                    )
                )
            buf = [sent]
            if estimate_tokens(sent) > TARGET:
                for part in _split_at_whitespace(sent, TARGET):
                    pieces.append(
                        _Block(
                            text=part,
                            page=block.page,
                            heading_trail=list(block.heading_trail),
                            bbox=block.bbox,
                        )
                    )
                buf = []
        else:
            buf.append(sent)
    if buf:
        for part in _split_at_whitespace(" ".join(buf), TARGET):
            pieces.append(
                _Block(
                    text=part,
                    page=block.page,
                    heading_trail=list(block.heading_trail),
                    bbox=block.bbox,
                )
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


def _heading_key(line: str) -> str:
    raw = line.strip()
    if raw.startswith("#"):
        m = _HEADING_RE.match(raw)
        raw = m.group(2) if m else raw.lstrip("#").strip()
    return re.sub(r"[^a-z0-9]", "", raw.lower())


def is_references_heading(line: str) -> bool:
    return _heading_key(line) in _REF_SECTION_KEYS


def citation_line_ratio(text: str) -> float:
    lines = [ln for ln in (text or "").splitlines() if ln.strip()]
    if not lines:
        return 0.0
    hits = sum(1 for ln in lines if _CITATION_LINE_RE.search(ln))
    return hits / len(lines)


def extract_chunk_labels(text: str) -> list[str]:
    labels: list[str] = []
    seen: set[str] = set()
    for ln in (text or "").splitlines():
        m = _EQ_LABEL_RE.match(ln)
        if m:
            label = m.group(1)
        else:
            m = _CAPTION_LABEL_RE.match(ln)
            if not m:
                continue
            label = m.group(1)
        if label in seen:
            continue
        seen.add(label)
        labels.append(label)
    return labels


def assign_chunk_role(
    text: str,
    heading_trail: list[str],
    *,
    after_references_heading: bool,
) -> str:
    if after_references_heading:
        return "references"
    for h in heading_trail:
        if is_references_heading(h):
            return "references"
    for ln in (text or "").splitlines()[:8]:
        if is_references_heading(ln):
            return "references"
    if citation_line_ratio(text) >= 0.4:
        return "references"
    return "body"


def _pack_contains_references_heading(pack: list[_Block], trail: list[str]) -> bool:
    if any(is_references_heading(h) for h in trail):
        return True
    for b in pack:
        for ln in b.text.splitlines():
            if is_references_heading(ln):
                return True
    return False


def _pack_blocks(blocks: list[_Block], file_type: str) -> list[ChunkDraft]:
    blocks = _expand_blocks(blocks)
    if not blocks:
        return []
    chunks: list[ChunkDraft] = []
    i = 0
    prev_body = ""
    after_refs = False
    use_trail = file_type in ("markdown", "docx")
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
        trail = pack[0].heading_trail if use_trail else []
        if _pack_contains_references_heading(pack, trail):
            after_refs = True
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
        role = assign_chunk_role(text, trail, after_references_heading=after_refs)
        regions = _regions_from_pack(pack) if file_type == "pdf" else None
        chunks.append(
            ChunkDraft(
                text=text,
                page_start=page_start,
                page_end=page_end,
                heading_trail=list(trail),
                role=role,
                regions=regions,
            )
        )
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
        pack_ft = "pdf"
    elif ft == "docx":
        blocks = _markdown_blocks(text if t else "")
        pack_ft = "docx"
    else:
        blocks = _markdown_blocks(text if t else "")
        pack_ft = "markdown"
    return _pack_blocks(blocks, pack_ft)
