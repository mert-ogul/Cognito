"""PDF scan labels: blank pages vs image pages vs titles."""

from __future__ import annotations

from pathlib import Path
from unittest import mock

import fitz

from ingestion.parsing import parse_file, _scan_status_from_pages, _title_is_junk


def _text_page(
    doc: fitz.Document, text: str = "Mission report paragraph about lunar module."
) -> None:
    page = doc.new_page(width=400, height=600)
    page.insert_textbox(fitz.Rect(40, 40, 360, 560), (text + " ") * 12)


def _blank_page(doc: fitz.Document) -> None:
    doc.new_page(width=400, height=600)


def _image_only_page(doc: fitz.Document) -> None:
    page = doc.new_page(width=400, height=600)
    # Solid pixmap covering the full page (counts as a large image).
    pix = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, 400, 600), False)
    pix.set_rect(pix.irect, (180, 180, 180))
    page.insert_image(page.rect, pixmap=pix)


def test_mostly_text_with_blank_pages_has_no_label(tmp_path: Path) -> None:
    pdf = tmp_path / "apollo_report.pdf"
    doc = fitz.open()
    for _ in range(50):
        _text_page(doc)
    _blank_page(doc)
    _blank_page(doc)
    doc.save(pdf)
    doc.close()

    with mock.patch("ingestion.parsing._ocr_page_darwin", return_value=("", [])):
        parsed = parse_file(pdf)
    assert parsed["scanStatus"] is None
    assert "Mission report" in parsed["text"]


def test_all_image_pages_no_ocr_text_is_no_text(tmp_path: Path) -> None:
    pdf = tmp_path / "photos_only.pdf"
    doc = fitz.open()
    for _ in range(4):
        _image_only_page(doc)
    doc.save(pdf)
    doc.close()

    with mock.patch("ingestion.parsing._ocr_page_darwin", return_value=("", [])):
        parsed = parse_file(pdf)
    assert parsed["scanStatus"] == "no_text"
    assert not (parsed.get("text") or "").strip()


def test_half_image_only_pages_partial_text(tmp_path: Path) -> None:
    pdf = tmp_path / "mixed.pdf"
    doc = fitz.open()
    for _ in range(4):
        _text_page(doc)
    for _ in range(4):
        _image_only_page(doc)
    doc.save(pdf)
    doc.close()

    with mock.patch("ingestion.parsing._ocr_page_darwin", return_value=("", [])):
        parsed = parse_file(pdf)
    assert parsed["scanStatus"] == "partial_text"
    assert "Mission report" in parsed["text"]


def test_letter_spaced_title_falls_back_to_filename(tmp_path: Path) -> None:
    assert _title_is_junk("p R E s s K")
    pdf = tmp_path / "Apollo_11_Mission_Report.pdf"
    doc = fitz.open()
    _text_page(doc, "Body text without a large title span.")
    doc.set_metadata({"title": "p R E s s K"})
    doc.save(pdf)
    doc.close()

    with mock.patch(
        "ingestion.parsing._largest_font_title",
        return_value="p R E s s K",
    ):
        parsed = parse_file(pdf)
    assert parsed["title"] == "Apollo 11 Mission Report"


def test_scan_status_helpers() -> None:
    assert _scan_status_from_pages([]) == "no_text"
    pages = [{"text": "hi", "scanned": False, "ocr": False, "blank": False} for _ in range(10)]
    assert _scan_status_from_pages(pages) is None
    pages[0] = {"text": "from ocr", "scanned": True, "ocr": True, "blank": False}
    # 1 of 10 is exactly 10%
    assert _scan_status_from_pages(pages) == "ocr"
