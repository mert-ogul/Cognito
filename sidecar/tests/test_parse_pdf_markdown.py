"""Parse PDF (PyMuPDF) and Markdown fixtures."""

from __future__ import annotations

from pathlib import Path

import fitz

from ingestion.parsing import parse_file, read_markdown_text


def _write_mini_pdf(path: Path) -> None:
    doc = fitz.open()
    doc.new_page()
    page = doc.load_page(0)
    page.insert_text((72, 72), "Hello PDF fixture")
    doc.save(path)
    doc.close()


def test_parse_markdown_fixture(tmp_path: Path) -> None:
    src = Path(__file__).resolve().parent / "fixtures" / "sample.md"
    dst = tmp_path / "sample.md"
    dst.write_text(src.read_text(encoding="utf-8"), encoding="utf-8")
    out = parse_file(dst)
    assert out["fileType"] == "markdown"
    assert "Section A" in out["text"]
    assert out["pageCount"] is None
    assert "pages" not in out


def test_markdown_text_includes_headings(tmp_path: Path) -> None:
    p = tmp_path / "h.md"
    p.write_text("# H1\n\n## H2\n\nbody\n", encoding="utf-8")
    text = read_markdown_text(p)
    assert "H2" in text and "body" in text


def test_parse_pdf_fixture(tmp_path: Path) -> None:
    pdf = tmp_path / "t.pdf"
    _write_mini_pdf(pdf)
    out = parse_file(pdf)
    assert out["fileType"] == "pdf"
    assert out["pageCount"] == 1
    assert "Hello PDF" in out["text"]
    assert len(out["contentHash"]) == 64
    assert out["pages"] == [{"page": 1, "text": out["pages"][0]["text"]}]
    assert "Hello PDF" in out["pages"][0]["text"]
