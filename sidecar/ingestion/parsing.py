"""PDF (PyMuPDF) and Markdown text extraction."""

from __future__ import annotations

import hashlib
import re
from pathlib import Path

import fitz  # PyMuPDF

_SMALL_CAPS_JOIN = re.compile(r"\b([A-Z]) ([A-Z]{2,})")
_DASH_JOIN = re.compile(r"(\w) -(\w)")
_TITLE_CASE_SMALL = frozenset(
    {
        "a",
        "an",
        "and",
        "as",
        "at",
        "by",
        "for",
        "in",
        "of",
        "on",
        "or",
        "the",
        "to",
        "with",
    }
)


def hash_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def clean_title(raw: str) -> str:
    text = re.sub(r"\s+", " ", (raw or "").strip())
    if not text:
        return ""
    prev = None
    while prev != text:
        prev = text
        text = _SMALL_CAPS_JOIN.sub(r"\1\2", text)
    text = _DASH_JOIN.sub(r"\1-\2", text)
    if text.isupper():
        words = text.split(" ")
        out: list[str] = []
        for i, w in enumerate(words):
            low = w.lower()
            if i > 0 and low in _TITLE_CASE_SMALL:
                out.append(low)
            else:
                parts = w.split("-")
                out.append(
                    "-".join(
                        (p[:1].upper() + p[1:].lower()) if p else p for p in parts
                    )
                )
        text = " ".join(out)
    return text


def _metadata_title_ok(title: str) -> bool:
    t = (title or "").strip()
    if len(t.split()) < 3:
        return False
    low = t.lower()
    if low.startswith("arxiv") or low.startswith("microsoft word"):
        return False
    if low.endswith(".doc") or low.endswith(".docx"):
        return False
    return True


def _largest_font_title(page: fitz.Page) -> str | None:
    data = page.get_text("dict") or {}
    best_size = -1.0
    for block in data.get("blocks") or []:
        for line in block.get("lines") or []:
            for span in line.get("spans") or []:
                size = float(span.get("size") or 0)
                if size > best_size:
                    best_size = size
    if best_size < 0:
        return None
    parts: list[str] = []
    for block in data.get("blocks") or []:
        for line in block.get("lines") or []:
            spans = line.get("spans") or []
            if not spans:
                continue
            if max(float(s.get("size") or 0) for s in spans) < best_size - 0.01:
                continue
            line_text = "".join(str(s.get("text") or "") for s in spans).strip()
            if not line_text or len(line_text) > 200:
                continue
            if "arXiv:" in line_text or "Published as" in line_text:
                continue
            parts.append(line_text)
    if not parts:
        return None
    return " ".join(parts)


def pdf_title(doc: fitz.Document, fallback: str) -> str:
    meta = doc.metadata or {}
    meta_title = str(meta.get("title") or "").strip()
    if _metadata_title_ok(meta_title):
        cleaned = clean_title(meta_title)
        if cleaned:
            return cleaned
    if len(doc) > 0:
        page_title = _largest_font_title(doc.load_page(0))
        if page_title:
            cleaned = clean_title(page_title)
            if cleaned:
                return cleaned
    return fallback


def markdown_title(text: str, fallback: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            cleaned = clean_title(line[2:])
            if cleaned:
                return cleaned
    return fallback


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
    fallback = path.stem
    if suf == ".pdf":
        doc = fitz.open(path)
        try:
            pages: list[dict] = []
            for i in range(len(doc)):
                page = doc.load_page(i)
                pages.append({"page": i + 1, "text": page.get_text("text") or ""})
            text = "\n\n".join(p["text"] for p in pages)
            title = pdf_title(doc, fallback)
            page_count = len(doc)
        finally:
            doc.close()
        return {
            "path": str(path.resolve()),
            "title": title,
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
            "title": markdown_title(text, fallback),
            "fileType": "markdown",
            "pageCount": None,
            "text": text,
            "contentHash": chash,
        }
    raise ValueError(f"unsupported file type: {suf}")
