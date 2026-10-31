"""NFKC ligatures + line-end hyphens for FTS keyword search."""

from __future__ import annotations

import sqlite3
import tempfile
import unicodedata
from pathlib import Path

from common.text_norm import fix_pdf_accents, normalize_search_text
from query.chat_pipeline import build_fts_query

ACUTE = chr(0xB4)
GRAVE = chr(0x60)
CARON = chr(0x2C7)
TILDE = chr(0x2DC)


def test_nfkc_turns_ligature_classification_into_ascii() -> None:
    assert normalize_search_text("classiﬁcation") == "classification"
    assert "fi" in normalize_search_text("ﬁlter")
    assert normalize_search_text("deﬁne") == "define"


def test_line_end_hyphen_joined() -> None:
    assert normalize_search_text("par-\nallelized") == "parallelized"
    assert "\u00ad" not in normalize_search_text("soft\u00adhyphen")


def test_fix_pdf_accents_before_nfkc() -> None:
    nfkc = lambda s: unicodedata.normalize("NFKC", s)
    assert nfkc(fix_pdf_accents(f"Barab{ACUTE}asi")) == nfkc("Barabási")
    assert nfkc(fix_pdf_accents(f"Veli{CARON}ckovi{ACUTE}c")) == nfkc("Veličković")
    assert nfkc(fix_pdf_accents(f"Li{GRAVE}o")) == nfkc("Liò")
    assert nfkc(fix_pdf_accents(f"Here, {TILDE}A")) == nfkc("Here, Ã")
    assert fix_pdf_accents("café ` code") == "café ` code"
    assert normalize_search_text("café naïve") == unicodedata.normalize("NFC", "café naïve")


def test_fts_finds_normalised_classification() -> None:
    d = tempfile.mkdtemp()
    path = str(Path(d) / "v.sqlite")
    con = sqlite3.connect(path)
    try:
        con.execute("CREATE VIRTUAL TABLE chunks_fts USING fts5(text, tokenize='porter unicode61')")
        raw = "We present a scalable approach to classiﬁcation with graph convolutions."
        con.execute(
            "INSERT INTO chunks_fts(text) VALUES (?)",
            (normalize_search_text(raw),),
        )
        con.commit()
        fts_q = build_fts_query("classification")
        row = con.execute(
            "SELECT text FROM chunks_fts WHERE chunks_fts MATCH ?",
            (fts_q,),
        ).fetchone()
        assert row is not None
        assert "classification" in row[0]
    finally:
        con.close()
