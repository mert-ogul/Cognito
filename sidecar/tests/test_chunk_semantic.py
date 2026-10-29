"""Natural-boundary semantic chunking."""

from __future__ import annotations

from pathlib import Path

import fitz

from ingestion.chunking import TARGET, chunk_semantic, estimate_tokens
from ingestion.parsing import parse_file


def test_chunk_semantic_multiple_chunks() -> None:
    text = "word " * 3000
    chunks = chunk_semantic(text, "markdown")
    assert len(chunks) >= 2
    for c in chunks:
        assert estimate_tokens(c.text) >= 1
        assert c.text == c.text.strip()


def test_no_chunk_splits_mid_word() -> None:
    text = "alpha beta gamma delta epsilon zeta eta theta " * 400
    chunks = chunk_semantic(text, "markdown")
    assert len(chunks) >= 2
    for c in chunks:
        s = c.text.strip()
        first = s.split()[0].strip("#>`")
        last = s.split()[-1].strip(".!?#>`")
        assert first.isalpha()
        assert last.isalpha()


def test_markdown_chunks_carry_heading_trail() -> None:
    md = (
        "# Setup\n\n## Install\n\n"
        + ("Step details here. " * 200)
        + "\n\n## Configure\n\n"
        + ("More configuration text. " * 80)
    )
    chunks = chunk_semantic(md, "markdown")
    assert chunks
    assert any("Setup > Install" in c.text for c in chunks)


def test_oversized_paragraph_splits_at_sentences() -> None:
    sentences = [f"Sentence number {i} is long enough to matter here." for i in range(200)]
    text = " ".join(sentences)
    chunks = chunk_semantic(text, "markdown")
    assert len(chunks) >= 2
    for c in chunks:
        assert estimate_tokens(c.text) <= TARGET + 80
        assert "Sentence number" in c.text


def test_pdf_pages_propagate_to_chunk_ranges(tmp_path: Path) -> None:
    pdf = tmp_path / "three.pdf"
    doc = fitz.open()
    for label in ["ALPHA_PAGE_ONE", "BETA_PAGE_TWO", "GAMMA_PAGE_THREE"]:
        page = doc.new_page()
        page.insert_text((72, 72), f"{label}. " + ("More words on this page. " * 5))
    doc.save(pdf)
    doc.close()

    parsed = parse_file(pdf)
    assert parsed["pageCount"] == 3
    assert len(parsed["pages"]) == 3
    chunks = chunk_semantic(parsed["text"], "pdf", pages=parsed["pages"])
    assert chunks
    for c in chunks:
        assert c.page_start is not None and c.page_end is not None
        assert 1 <= c.page_start <= c.page_end <= 3
    for label, page in {
        "ALPHA_PAGE_ONE": 1,
        "BETA_PAGE_TWO": 2,
        "GAMMA_PAGE_THREE": 3,
    }.items():
        matching = [c for c in chunks if label in c.text]
        assert matching, f"missing chunks for {label}"
        for c in matching:
            assert c.page_start <= page <= c.page_end
