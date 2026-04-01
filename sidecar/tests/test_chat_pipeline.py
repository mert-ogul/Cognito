"""prepare_chat_retrieval: vector-only when no graph seeds; structured output."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import pytest

from common.sqlite_vec_util import MOCK_EMBED_ENV
from query.chat_pipeline import prepare_chat_retrieval


@pytest.fixture
def mock_embed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(MOCK_EMBED_ENV, "1")


def test_prepare_empty_library_messages(mock_embed: None) -> None:
    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    duck = str(Path(d) / "g.duckdb")
    sq = sqlite3.connect(sqlite_path)
    sq.executescript(
        """
        CREATE TABLE documents (id TEXT, path TEXT, title TEXT, status TEXT);
        CREATE TABLE chunks (id TEXT, document_id TEXT, ordinal INT, text TEXT, page_start INT, page_end INT);
        """
    )
    sq.commit()
    sq.close()
    out = prepare_chat_retrieval(sqlite_path, duck, "hello", "nomic-embed-text")
    assert out["retrievalMode"] == "vector_only"
    assert "messages" in out


def test_prepare_query_embedding_failed_when_chunks_exist_but_no_hits(
    mock_embed: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Simulate embed failure (no query vector) while library has chunks: explicit vector_only guidance."""
    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    sq = sqlite3.connect(sqlite_path)
    sq.executescript("CREATE TABLE chunks (id TEXT); INSERT INTO chunks VALUES ('c1');")
    sq.commit()
    sq.close()

    import query.chat_pipeline as cp

    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: [])
    out = cp.prepare_chat_retrieval(sqlite_path, None, "hello", "nomic-embed-text")
    assert out["retrievalMode"] == "vector_only"
    assert out.get("queryEmbeddingFailed") is True
    assert "Ollama" in out["messages"][0]["content"]


def test_search_chunks_rpc_shape(mock_embed: None) -> None:
    from query.chat_pipeline import rpc_search_chunks

    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    sq = sqlite3.connect(sqlite_path)
    sq.executescript("CREATE TABLE documents (id TEXT);")
    sq.commit()
    sq.close()
    r = rpc_search_chunks({"sqlite_path": sqlite_path, "query": "x", "k": 3})
    assert "chunks" in r
    assert r["chunks"] == []
