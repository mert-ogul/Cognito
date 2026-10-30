"""NFKC ligatures + line-end hyphens for FTS keyword search."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

from common.text_norm import normalize_search_text
from query.chat_pipeline import build_fts_query


def test_nfkc_turns_ligature_classification_into_ascii() -> None:
    assert normalize_search_text("classiﬁcation") == "classification"
    assert "fi" in normalize_search_text("ﬁlter")
    assert normalize_search_text("deﬁne") == "define"


def test_line_end_hyphen_joined() -> None:
    assert normalize_search_text("par-\nallelized") == "parallelized"
    assert "\u00ad" not in normalize_search_text("soft\u00adhyphen")


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
