"""Exact-token fusion, text-quality gate, and rerank pool size."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import pytest

from common.sqlite_vec_util import MOCK_EMBED_ENV
from ingestion.chunking import TEXT_QUALITY_MIN, chunk_is_searchable, chunk_text_quality
from query.chat_pipeline import (
    _RERANK_POOL,
    exact_token_top_k,
    extract_rare_tokens,
    keyword_top_k,
    reciprocal_rank_fusion,
)


@pytest.fixture(autouse=True)
def mock_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(MOCK_EMBED_ENV, "1")
    monkeypatch.setenv("COGNITO_MOCK_MODELS", "1")


def test_extract_rare_tokens_codes_and_numbers() -> None:
    toks = extract_rare_tokens("What caused the 1202 and 1201 alarms on AS-204 and MSC-00171?")
    assert "1202" in toks
    assert "1201" in toks
    assert "AS-204" in toks
    assert "MSC-00171" in toks


def test_chunk_text_quality_rejects_ocr_junk() -> None:
    prose = (
        "The Executive overflow alarms that occurred during descent are now known "
        "to be normal computer behaviour under radar load."
    )
    junk = (
        "\x1a\x17-'\x14/\u0601\u0bad\u0c49\x04\x01 &\x13 /#\x18!)!\x17'\x0e$\x18/!\x15/ "
        '\x10\x1a \x16/(\x1a*\x14/\x14 \x1b%(\x14/(\x19".\x1b \x16/\x16&",'
    )
    assert chunk_text_quality(prose) >= TEXT_QUALITY_MIN
    assert chunk_is_searchable(prose)
    assert chunk_text_quality(junk) < TEXT_QUALITY_MIN
    assert not chunk_is_searchable(junk)


def test_rerank_pool_is_thirty() -> None:
    assert _RERANK_POOL == 30


def _retrieval_fixture() -> str:
    d = tempfile.mkdtemp()
    path = str(Path(d) / "v.sqlite")
    con = sqlite3.connect(path)
    try:
        con.executescript(
            """
            CREATE TABLE documents (
              id TEXT PRIMARY KEY, title TEXT, path TEXT, status TEXT
            );
            CREATE TABLE chunks (
              id TEXT PRIMARY KEY,
              document_id TEXT,
              ordinal INT,
              text TEXT,
              page_start INT,
              page_end INT,
              role TEXT,
              labels TEXT,
              regions TEXT,
              text_quality REAL
            );
            CREATE VIRTUAL TABLE chunks_fts USING fts5(
              text,
              content='chunks',
              content_rowid='rowid',
              tokenize='porter unicode61 remove_diacritics 2'
            );
            CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
              INSERT INTO chunks_fts(rowid, text) VALUES (new.rowid, new.text);
            END;
            """
        )
        con.execute(
            "INSERT INTO documents VALUES (?,?,?,?)",
            ("d_alarm", "Apollo 11 transcript", "/a11.pdf", "indexed"),
        )
        con.execute(
            "INSERT INTO documents VALUES (?,?,?,?)",
            ("d_other", "Unrelated notes", "/notes.md", "indexed"),
        )
        con.execute(
            "INSERT INTO chunks VALUES (?,?,?,?,?,?,?,?,?,?)",
            (
                "c_1202",
                "d_alarm",
                0,
                "04 06 38 LMP Houston. I'm getting a 1202 alarm.",
                277,
                277,
                "body",
                "[]",
                None,
                0.8,
            ),
        )
        con.execute(
            "INSERT INTO chunks VALUES (?,?,?,?,?,?,?,?,?,?)",
            (
                "c_vague",
                "d_other",
                0,
                "Mission Control decided many things during landings over the years.",
                1,
                1,
                "body",
                "[]",
                None,
                0.9,
            ),
        )
        con.execute(
            "INSERT INTO chunks VALUES (?,?,?,?,?,?,?,?,?,?)",
            (
                "c_junk",
                "d_other",
                1,
                "\x1a\x17-'\x14/\u0601\u0bad junk figure page It~>vcf==o/1LJ..",
                82,
                82,
                "body",
                "[]",
                None,
                0.2,
            ),
        )
        con.commit()
    finally:
        con.close()
    return path


def test_exact_token_list_ranks_code_chunk_first() -> None:
    path = _retrieval_fixture()
    q = "What caused the 1202 alarm during landing?"
    exact = exact_token_top_k(path, q, 10)
    assert exact
    assert exact[0]["chunk_id"] == "c_1202"
    keyword = keyword_top_k(path, q, 10)
    fused = reciprocal_rank_fusion([keyword, exact])
    assert fused[0]["chunk_id"] == "c_1202"


def test_keyword_skips_low_text_quality_chunks() -> None:
    path = _retrieval_fixture()
    hits = keyword_top_k(path, "junk figure page", 10)
    ids = {h["chunk_id"] for h in hits}
    assert "c_junk" not in ids
