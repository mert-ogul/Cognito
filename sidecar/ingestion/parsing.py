"""PDF (PyMuPDF) and Markdown text extraction with heading context."""

from __future__ import annotations

import hashlib
import re
from pathlib import Path

import fitz  # PyMuPDF


def hash_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_pdf_text(path: Path) -> tuple[str, int]:
    doc = fitz.open(path)
    try:
        parts: list[str] = []
        for i in range(len(doc)):
            page = doc.load_page(i)
            parts.append(page.get_text("text") or "")
        return "\n\n".join(parts), len(doc)
    finally:
        doc.close()


def read_markdown_with_context(path: Path) -> tuple[str, int | None]:
    raw = path.read_text(encoding="utf-8", errors="replace")
    lines = raw.splitlines()
    sections: list[str] = []
    trail: list[str] = []
    for line in lines:
        if re.match(r"^#{1,6}\s+", line):
            if trail:
                sections.append("\n".join(trail))
                trail = []
            trail.append(line)
        else:
            trail.append(line)
    if trail:
        sections.append("\n".join(trail))
    merged = "\n\n".join(s for s in sections if s.strip())
    if not merged.strip():
        merged = raw
    return merged, None


def parse_file(path: Path) -> dict:
    raw = path.read_bytes()
    chash = hash_bytes(raw)
    suf = path.suffix.lower()
    if suf == ".pdf":
        text, pages = read_pdf_text(path)
        ftype = "pdf"
        page_count: int | None = pages
    elif suf in (".md", ".markdown"):
        text, _ = read_markdown_with_context(path)
        ftype = "markdown"
        page_count = None
    else:
        raise ValueError(f"unsupported file type: {suf}")
    return {
        "path": str(path.resolve()),
        "title": path.stem,
        "fileType": ftype,
        "pageCount": page_count,
        "text": text,
        "contentHash": chash,
    }
