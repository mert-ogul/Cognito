"""PDF (PyMuPDF) and Markdown text extraction."""

from __future__ import annotations

import hashlib
from pathlib import Path

import fitz  # PyMuPDF


def hash_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_pdf_text(path: Path) -> tuple[str, list[dict], int]:
    doc = fitz.open(path)
    try:
        pages: list[dict] = []
        for i in range(len(doc)):
            page = doc.load_page(i)
            pages.append({"page": i + 1, "text": page.get_text("text") or ""})
        text = "\n\n".join(p["text"] for p in pages)
        return text, pages, len(doc)
    finally:
        doc.close()


def read_markdown_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def parse_file(path: Path) -> dict:
    raw = path.read_bytes()
    chash = hash_bytes(raw)
    suf = path.suffix.lower()
    if suf == ".pdf":
        text, pages, page_count = read_pdf_text(path)
        return {
            "path": str(path.resolve()),
            "title": path.stem,
            "fileType": "pdf",
            "pageCount": page_count,
            "text": text,
            "pages": pages,
            "contentHash": chash,
        }
    if suf in (".md", ".markdown"):
        text = read_markdown_text(path)
        return {
            "path": str(path.resolve()),
            "title": path.stem,
            "fileType": "markdown",
            "pageCount": None,
            "text": text,
            "contentHash": chash,
        }
    raise ValueError(f"unsupported file type: {suf}")
