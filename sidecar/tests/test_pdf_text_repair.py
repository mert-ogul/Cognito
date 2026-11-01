"""Letter-spaced PDF text repair and OCR fallback."""

from __future__ import annotations

from pathlib import Path
from unittest import mock

import fitz

from ingestion.parsing import parse_file, text_quality


def _letter_spaced_page(doc: fitz.Document, words: list[str]) -> None:
    """Old OCR style: a space after every letter, wider gaps between words."""
    page = doc.new_page(width=620, height=200)
    parts: list[str] = []
    for wi, word in enumerate(words):
        if wi:
            parts.append("   ")  # word separator (ignored in repair; widens glyph gaps)
        parts.append(" ".join(word))
    page.insert_text((40, 80), "".join(parts), fontsize=12)


def test_letter_spaced_page_repairs_to_words(tmp_path: Path) -> None:
    pdf = tmp_path / "spaced.pdf"
    doc = fitz.open()
    _letter_spaced_page(doc, ["First", "of", "the", "Apollo", "mission"])
    doc.save(pdf)
    doc.close()

    # Confirm the raw extract looks letter-spaced before repair.
    raw_doc = fitz.open(pdf)
    raw = raw_doc[0].get_text("text")
    raw_doc.close()
    single, _ = text_quality(raw)
    assert single > 0.3

    with mock.patch("ingestion.parsing._ocr_page_darwin", return_value=("", [])):
        parsed = parse_file(pdf)
    text = parsed["text"]
    assert "First" in text
    assert "Apollo" in text
    assert "mission" in text
    assert parsed["pages"][0].get("repaired") is True
    single_after, _ = text_quality(text)
    assert single_after <= 0.3


def test_normal_page_unchanged(tmp_path: Path) -> None:
    pdf = tmp_path / "normal.pdf"
    doc = fitz.open()
    page = doc.new_page(width=400, height=400)
    page.insert_textbox(
        fitz.Rect(40, 40, 360, 360),
        "The Apollo 11 mission landed on the Moon in July 1969. " * 4,
    )
    doc.save(pdf)
    doc.close()

    with mock.patch("ingestion.parsing._ocr_page_darwin", return_value=("", [])):
        parsed = parse_file(pdf)
    assert parsed["pages"][0].get("repaired") is False
    assert parsed["pages"][0].get("ocr") is False
    assert "Apollo 11 mission" in parsed["text"]


def test_dots_only_page_uses_ocr_when_mocked(tmp_path: Path) -> None:
    pdf = tmp_path / "dots.pdf"
    doc = fitz.open()
    page = doc.new_page(width=400, height=200)
    page.insert_text((40, 80), "................ " * 20, fontsize=12)
    doc.save(pdf)
    doc.close()

    ocr_blocks = [{"text": "Recovered OCR sentence.", "bbox": [10.0, 10.0, 200.0, 40.0]}]
    with mock.patch(
        "ingestion.parsing.platform.system",
        return_value="Darwin",
    ), mock.patch(
        "ingestion.parsing._ocr_page_darwin",
        return_value=("Recovered OCR sentence.", ocr_blocks),
    ):
        parsed = parse_file(pdf)
    assert parsed["pages"][0].get("ocr") is True
    assert "Recovered OCR sentence" in parsed["text"]
