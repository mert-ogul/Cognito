"""PDF (PyMuPDF), Word (.docx), and Markdown text extraction."""

from __future__ import annotations

import hashlib
import logging
import platform
import re
import statistics
import tempfile
from collections import Counter
from pathlib import Path

import fitz  # PyMuPDF

from common.text_norm import normalize_pdf_text, normalize_search_text

log = logging.getLogger("cognito.sidecar")

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
_SCANNED_TEXT_MIN = 20
_HEADING_STYLE_RE = re.compile(r"^heading\s*(\d+)$", re.IGNORECASE)
_SINGLE_LETTER_OK = frozenset({"a", "A", "I"})
_VOWELS = frozenset("aeiouyAEIOUY")
_LETTER_SPACED_SHARE = 0.3
_LETTER_CHAR_MIN = 0.3
_ARXIV_STEM_RE = re.compile(r"^\d{4}\.\d+")
_CODE_PREFIX_RE = re.compile(r"^[A-Za-z]\d+$")


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
                out.append("-".join((p[:1].upper() + p[1:].lower()) if p else p for p in parts))
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
    return not _title_is_junk(t)


def _title_is_junk(title: str) -> bool:
    """Reject letter-spaced, punctuation-heavy, or one-character-dominated titles."""
    t = (title or "").strip()
    if len(t) < 3:
        return True
    letters = sum(1 for c in t if c.isalpha())
    if letters / len(t) < 0.6:
        return True
    alpha_words = re.findall(r"[A-Za-z]+", t)
    if not any(len(w) >= 3 and any(c in _VOWELS for c in w) for w in alpha_words):
        return True
    tokens = t.split()
    if tokens:
        single = sum(1 for tok in tokens if len(tok) == 1)
        if single > len(tokens) / 2:
            return True
    counts = Counter(c.lower() for c in t if not c.isspace())
    total = sum(counts.values())
    if total and counts.most_common(1)[0][1] / total > 0.4:
        return True
    return False


def _title_candidate_ok(title: str) -> bool:
    return not _title_is_junk(title)


def _filename_display(stem: str) -> str:
    spaced = stem.replace("_", " ").replace("-", " ").strip()
    cleaned = clean_title(spaced)
    return cleaned or spaced


def _filename_title(path: Path) -> str:
    return _filename_display(path.stem)


def _filename_reads_like_name(stem: str) -> bool:
    """True when the stem looks like a human title, not a code or arXiv id."""
    raw = (stem or "").strip()
    if not raw or _ARXIV_STEM_RE.match(raw):
        return False
    parts = re.split(r"[_\-\s]+", raw)
    if parts and _CODE_PREFIX_RE.match(parts[0] or ""):
        return False
    spaced = _filename_display(raw)
    letter_words = [w for w in re.findall(r"[A-Za-z]+", spaced) if len(w) >= 2]
    if len(letter_words) < 2:
        return False
    digits = sum(1 for c in raw if c.isdigit())
    if digits > len(raw) * 0.3:
        return False
    return not _title_is_junk(spaced)


def _line_upright(line: dict) -> bool:
    direction = line.get("dir") or [1, 0]
    try:
        dx = float(direction[0])
        dy = float(direction[1])
    except (TypeError, ValueError, IndexError):
        return True
    return abs(dy) < 0.15 and dx > 0.85


def _line_y0(line: dict) -> float:
    bbox = line.get("bbox")
    if bbox and len(bbox) >= 2:
        try:
            return float(bbox[1])
        except (TypeError, ValueError):
            pass
    for span in line.get("spans") or []:
        sb = span.get("bbox")
        if sb and len(sb) >= 2:
            try:
                return float(sb[1])
            except (TypeError, ValueError):
                continue
    return 0.0


def _largest_font_title(page: fitz.Page) -> str | None:
    data = page.get_text("dict") or {}
    page_h = float(page.rect.height) or 1.0
    y_limit = 0.6 * page_h
    best_size = -1.0
    for block in data.get("blocks") or []:
        for line in block.get("lines") or []:
            if not _line_upright(line):
                continue
            if _line_y0(line) >= y_limit:
                continue
            spans = line.get("spans") or []
            if not spans:
                continue
            line_text = "".join(str(s.get("text") or "") for s in spans)
            if "arXiv:" in line_text:
                continue
            for span in spans:
                size = float(span.get("size") or 0)
                if size > best_size:
                    best_size = size
    if best_size < 0:
        return None
    parts: list[str] = []
    for block in data.get("blocks") or []:
        for line in block.get("lines") or []:
            if not _line_upright(line):
                continue
            if _line_y0(line) >= y_limit:
                continue
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


def pdf_title(doc: fitz.Document, path: Path) -> str:
    """Pick a readable title: metadata → filename if name-like → page-1 font → spaced filename."""
    meta = doc.metadata or {}
    meta_title = str(meta.get("title") or "").strip()
    if _metadata_title_ok(meta_title):
        cleaned = clean_title(meta_title)
        if cleaned and not _title_is_junk(cleaned):
            return cleaned
    stem = path.stem
    if _filename_reads_like_name(stem):
        return _filename_display(stem)
    if len(doc) > 0:
        page_title = _largest_font_title(doc.load_page(0))
        if page_title:
            cleaned = clean_title(page_title)
            if cleaned and not _title_is_junk(cleaned):
                return cleaned
    return _filename_display(stem)


def markdown_title(text: str, fallback: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            cleaned = clean_title(line[2:])
            if cleaned:
                return cleaned
    return fallback


def _text_block_from_dict(block: dict) -> dict | None:
    """Build `{text, bbox}` from a PyMuPDF dict text block (type 0)."""
    raw_type = block.get("type")
    try:
        block_type = int(raw_type) if raw_type is not None else -1
    except (TypeError, ValueError):
        return None
    if block_type != 0:
        return None
    parts: list[str] = []
    for line in block.get("lines") or []:
        spans = line.get("spans") or []
        line_text = "".join(str(s.get("text") or "") for s in spans).strip()
        if line_text:
            parts.append(line_text)
    text = "\n".join(parts).strip()
    if not text:
        return None
    bbox = block.get("bbox")
    if not bbox or len(bbox) < 4:
        return None
    try:
        box = [float(bbox[0]), float(bbox[1]), float(bbox[2]), float(bbox[3])]
    except (TypeError, ValueError):
        return None
    return {"text": normalize_pdf_text(text), "bbox": box}


def _ocr_page_darwin(page: fitz.Page) -> tuple[str, list[dict]]:
    """Run macOS Vision OCR via ocrmac; return text and block-like regions in page coords."""
    try:
        from ocrmac import ocrmac  # type: ignore
    except ImportError:
        return "", []

    pix = page.get_pixmap(matrix=fitz.Matrix(2, 2), alpha=False)
    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as tmp:
        tmp_path = Path(tmp.name)
    try:
        pix.save(str(tmp_path))
        annotations = ocrmac.OCR(str(tmp_path), recognition_level="accurate").recognize()
    except Exception:  # noqa: BLE001
        return "", []
    finally:
        try:
            tmp_path.unlink(missing_ok=True)
        except OSError:
            pass

    page_w = float(page.rect.width) or 1.0
    page_h = float(page.rect.height) or 1.0
    lines: list[str] = []
    blocks: list[dict] = []
    for item in annotations or []:
        # ocrmac returns (text, confidence, [x, y, w, h]) in normalized coords.
        if not isinstance(item, (list, tuple)) or len(item) < 3:
            continue
        text = str(item[0] or "").strip()
        box = item[2]
        if not text or not isinstance(box, (list, tuple)) or len(box) < 4:
            continue
        try:
            x, y, w, h = (float(box[0]), float(box[1]), float(box[2]), float(box[3]))
        except (TypeError, ValueError):
            continue
        # Vision uses bottom-left origin with normalized values; convert to PDF top-left.
        x0 = x * page_w
        x1 = (x + w) * page_w
        y0 = (1.0 - y - h) * page_h
        y1 = (1.0 - y) * page_h
        norm = normalize_pdf_text(text)
        lines.append(norm)
        blocks.append({"text": norm, "bbox": [x0, y0, x1, y1]})
    return "\n".join(lines).strip(), blocks


def _page_has_large_image(page: fitz.Page) -> bool:
    """True when an image covers at least half the page area."""
    page_area = float(page.rect.width) * float(page.rect.height)
    if page_area <= 0:
        return False
    threshold = 0.5 * page_area
    try:
        infos = page.get_image_info() or []
    except Exception:  # noqa: BLE001
        return False
    for info in infos:
        bbox = info.get("bbox")
        if not bbox or len(bbox) < 4:
            continue
        try:
            x0, y0, x1, y1 = (float(bbox[0]), float(bbox[1]), float(bbox[2]), float(bbox[3]))
        except (TypeError, ValueError):
            continue
        if (x1 - x0) * (y1 - y0) >= threshold:
            return True
    return False


def text_quality(text: str) -> tuple[float, float]:
    """Return (single_letter_share, letter_char_share) for alphabetic quality checks."""
    raw = text or ""
    tokens = re.findall(r"[A-Za-z]+", raw)
    if not tokens:
        single_share = 1.0 if raw.strip() else 0.0
    else:
        singles = sum(1 for t in tokens if len(t) == 1 and t not in _SINGLE_LETTER_OK)
        single_share = singles / len(tokens)
    if not raw:
        letter_share = 0.0
    else:
        letter_share = sum(1 for c in raw if c.isalpha()) / len(raw)
    return single_share, letter_share


def _repair_line_from_chars(line: dict) -> str:
    """Rebuild a line from character positions, ignoring the PDF's own spaces."""
    chars: list[tuple[float, float, str, float]] = []
    for span in line.get("spans") or []:
        try:
            size = float(span.get("size") or 10.0)
        except (TypeError, ValueError):
            size = 10.0
        for ch in span.get("chars") or []:
            c = str(ch.get("c") or "")
            if not c or c.isspace():
                continue
            bbox = ch.get("bbox")
            if not bbox or len(bbox) < 4:
                continue
            try:
                x0, _y0, x1, _y1 = (
                    float(bbox[0]),
                    float(bbox[1]),
                    float(bbox[2]),
                    float(bbox[3]),
                )
            except (TypeError, ValueError):
                continue
            chars.append((x0, x1, c, size))
    if not chars:
        return ""
    if len(chars) == 1:
        return chars[0][2]
    gaps = [chars[i + 1][0] - chars[i][1] for i in range(len(chars) - 1)]
    median_gap = statistics.median(gaps) if gaps else 0.0
    words: list[str] = []
    current = [chars[0][2]]
    for i, gap in enumerate(gaps):
        size = chars[i][3]
        if gap > median_gap + 0.25 * size:
            words.append("".join(current))
            current = [chars[i + 1][2]]
        else:
            current.append(chars[i + 1][2])
    words.append("".join(current))
    return " ".join(w for w in words if w)


def repair_page_from_rawdict(page: fitz.Page) -> tuple[str, list[dict]]:
    """Rebuild page text from character gaps; keep original block bboxes."""
    raw = page.get_text("rawdict") or {}
    blocks_out: list[dict] = []
    text_parts: list[str] = []
    for block in raw.get("blocks") or []:
        try:
            block_type = int(block.get("type")) if block.get("type") is not None else -1
        except (TypeError, ValueError):
            continue
        if block_type != 0:
            continue
        line_texts: list[str] = []
        for line in block.get("lines") or []:
            lt = _repair_line_from_chars(line)
            if lt:
                line_texts.append(lt)
        if not line_texts:
            continue
        block_text = normalize_pdf_text("\n".join(line_texts))
        text_parts.append(block_text)
        bbox = block.get("bbox")
        if bbox and len(bbox) >= 4:
            try:
                box = [float(bbox[0]), float(bbox[1]), float(bbox[2]), float(bbox[3])]
            except (TypeError, ValueError):
                continue
            blocks_out.append({"text": block_text, "bbox": box})
    return normalize_pdf_text("\n\n".join(text_parts)), blocks_out


def _try_ocr_page(page: fitz.Page) -> tuple[str, list[dict]] | None:
    if platform.system() != "Darwin":
        return None
    ocr_text, ocr_blocks = _ocr_page_darwin(page)
    if not ocr_text:
        return None
    return normalize_pdf_text(ocr_text), ocr_blocks


def _parse_pdf_page(page: fitz.Page, page_no: int) -> dict:
    plain = normalize_pdf_text((page.get_text("text") or "").strip())
    if len(plain) < _SCANNED_TEXT_MIN:
        if not _page_has_large_image(page):
            return {
                "page": page_no,
                "text": "",
                "blocks": [],
                "scanned": False,
                "ocr": False,
                "repaired": False,
                "blank": True,
            }
        ocr = _try_ocr_page(page)
        if ocr:
            return {
                "page": page_no,
                "text": ocr[0],
                "blocks": ocr[1],
                "scanned": True,
                "ocr": True,
                "repaired": False,
            }
        return {
            "page": page_no,
            "text": "",
            "blocks": [],
            "scanned": True,
            "ocr": False,
            "repaired": False,
        }

    data = page.get_text("dict") or {}
    blocks: list[dict] = []
    for block in data.get("blocks") or []:
        item = _text_block_from_dict(block)
        if item:
            blocks.append(item)
    text = normalize_pdf_text("\n\n".join(b["text"] for b in blocks) if blocks else plain)
    single_share, letter_share = text_quality(text)
    repaired = False

    if single_share > _LETTER_SPACED_SHARE:
        text, blocks = repair_page_from_rawdict(page)
        repaired = True
        single_share, letter_share = text_quality(text)

    if single_share > _LETTER_SPACED_SHARE or letter_share < _LETTER_CHAR_MIN:
        ocr = _try_ocr_page(page)
        if ocr:
            return {
                "page": page_no,
                "text": ocr[0],
                "blocks": ocr[1],
                "scanned": True,
                "ocr": True,
                "repaired": repaired,
            }

    return {
        "page": page_no,
        "text": text,
        "blocks": blocks,
        "scanned": False,
        "ocr": False,
        "repaired": repaired,
    }


def _scan_status_from_pages(pages: list[dict]) -> str | None:
    n = len(pages)
    if n == 0:
        return "no_text"
    joined = "\n\n".join(str(p.get("text") or "") for p in pages).strip()
    if not joined:
        return "no_text"

    ocr_pages = sum(1 for p in pages if p.get("ocr") and str(p.get("text") or "").strip())
    scanned_unreadable = sum(
        1 for p in pages if p.get("scanned") and not str(p.get("text") or "").strip()
    )
    native_text_pages = sum(
        1
        for p in pages
        if not p.get("scanned")
        and not p.get("blank")
        and len(str(p.get("text") or "").strip()) >= _SCANNED_TEXT_MIN
    )

    if ocr_pages >= max(1, int(0.1 * n)) or (native_text_pages == 0 and ocr_pages > 0):
        return "ocr"
    if scanned_unreadable > n / 4:
        return "partial_text"
    return None


def read_pdf_text(path: Path) -> tuple[str, list[dict], int]:
    doc = fitz.open(path)
    try:
        pages: list[dict] = []
        for i in range(len(doc)):
            pages.append(_parse_pdf_page(doc.load_page(i), i + 1))
        text = "\n\n".join(p["text"] for p in pages if p.get("text"))
        return text, pages, len(doc)
    finally:
        doc.close()


def read_markdown_text(path: Path) -> str:
    return normalize_search_text(path.read_text(encoding="utf-8", errors="replace"))


def _heading_level_from_style(style_name: str) -> int | None:
    name = (style_name or "").strip()
    m = _HEADING_STYLE_RE.match(name)
    if not m:
        return None
    level = int(m.group(1))
    if 1 <= level <= 6:
        return level
    return None


def parse_docx(path: Path) -> dict:
    from docx import Document  # type: ignore
    from docx.table import Table  # type: ignore
    from docx.text.paragraph import Paragraph  # type: ignore

    doc = Document(str(path))
    lines: list[str] = []

    def emit_paragraph(para: Paragraph) -> None:
        text = (para.text or "").strip()
        if not text:
            return
        style_name = ""
        try:
            style_name = str(para.style.name) if para.style is not None else ""
        except Exception:  # noqa: BLE001
            style_name = ""
        level = _heading_level_from_style(style_name)
        if level is not None:
            lines.append("#" * level + " " + text)
        else:
            lines.append(text)

    def emit_table(table: Table) -> None:
        for row in table.rows:
            cells = [(c.text or "").replace("\n", " ").strip() for c in row.cells]
            if any(cells):
                lines.append("\t".join(cells))

    body = doc.element.body
    for child in body.iterchildren():
        tag = child.tag.split("}")[-1] if "}" in child.tag else child.tag
        if tag == "p":
            emit_paragraph(Paragraph(child, doc))
        elif tag == "tbl":
            emit_table(Table(child, doc))

    text = normalize_search_text("\n\n".join(lines).strip())
    title = markdown_title(text, path.stem)
    return {
        "path": str(path.resolve()),
        "title": title,
        "fileType": "docx",
        "pageCount": None,
        "text": text,
        "contentHash": hash_bytes(path.read_bytes()),
        "scanStatus": None,
    }


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
                pages.append(_parse_pdf_page(doc.load_page(i), i + 1))
            text = "\n\n".join(p["text"] for p in pages if p.get("text"))
            title = pdf_title(doc, path)
            page_count = len(doc)
        finally:
            doc.close()
        repaired_n = sum(1 for p in pages if p.get("repaired"))
        ocr_n = sum(1 for p in pages if p.get("ocr"))
        if repaired_n or ocr_n:
            log.info(
                "pdf %s: repaired_pages=%s ocr_pages=%s total=%s",
                path.name,
                repaired_n,
                ocr_n,
                page_count,
            )
        scan_status = _scan_status_from_pages(pages)
        out = {
            "path": str(path.resolve()),
            "title": title,
            "fileType": "pdf",
            "pageCount": page_count,
            "text": text,
            "pages": pages,
            "contentHash": chash,
            "scanStatus": scan_status,
            "repairedPages": repaired_n,
            "ocrPages": ocr_n,
        }
        if scan_status == "ocr":
            out["textSource"] = "ocr"
        return out
    if suf in (".md", ".markdown"):
        text = read_markdown_text(path)
        return {
            "path": str(path.resolve()),
            "title": markdown_title(text, fallback),
            "fileType": "markdown",
            "pageCount": None,
            "text": text,
            "contentHash": chash,
            "scanStatus": None,
        }
    if suf == ".docx":
        parsed = parse_docx(path)
        parsed["contentHash"] = chash
        return parsed
    raise ValueError(f"unsupported file type: {suf}")
