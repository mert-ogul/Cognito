"""prepare_chat_retrieval: scoped hybrid retrieval, citations, history embed."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import pytest

from common.sqlite_vec_util import MOCK_EMBED_ENV
from query.chat_pipeline import embed_query_text, prepare_chat_retrieval, prepare_history

_MOCK_MODELS = "COGNITO_MOCK_MODELS"


@pytest.fixture
def mock_embed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(MOCK_EMBED_ENV, "1")
    monkeypatch.setenv(_MOCK_MODELS, "1")


def test_prepare_empty_library_not_found(mock_embed: None) -> None:
    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    sq = sqlite3.connect(sqlite_path)
    sq.executescript(
        """
        CREATE TABLE documents (id TEXT, path TEXT, title TEXT, status TEXT);
        CREATE TABLE chunks (
          id TEXT, document_id TEXT, ordinal INT, text TEXT,
          page_start INT, page_end INT, role TEXT, labels TEXT
        );
        """
    )
    sq.commit()
    sq.close()
    out = prepare_chat_retrieval(sqlite_path, "hello", "nomic-embed-text")
    assert out["retrievalMode"] == "vector_only"
    assert out["notFound"] is True
    assert out["citations"] == []
    assert "messages" not in out


def test_prepare_query_embedding_failed_when_chunks_exist_but_no_hits(
    mock_embed: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    sq = sqlite3.connect(sqlite_path)
    sq.executescript("CREATE TABLE chunks (id TEXT); INSERT INTO chunks VALUES ('c1');")
    sq.commit()
    sq.close()

    import query.chat_pipeline as cp

    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: [])
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    out = cp.prepare_chat_retrieval(sqlite_path, "hello", "nomic-embed-text")
    assert out["retrievalMode"] == "vector_only"
    assert out.get("queryEmbeddingFailed") is True
    assert out["notFound"] is True
    assert "messages" not in out


def _fake_chunks() -> list[dict]:
    return [
        {
            "chunk_id": "c1",
            "document_id": "d1",
            "text": "Machine learning systems learn from data.",
            "page_start": 1,
            "page_end": 1,
            "title": "ML",
            "path": "/ml.md",
            "role": "body",
            "labels": [],
        },
        {
            "chunk_id": "c2",
            "document_id": "d1",
            "text": "Neural nets are related.",
            "page_start": 2,
            "page_end": 2,
            "title": "ML",
            "path": "/ml.md",
            "role": "body",
            "labels": [],
        },
    ]


def test_citations_carry_index_no_messages(
    mock_embed: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    import query.chat_pipeline as cp

    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: _fake_chunks())
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    d = tempfile.mkdtemp()
    out = cp.prepare_chat_retrieval(str(Path(d) / "v.sqlite"), "what is ml", "nomic-embed-text")
    assert "messages" not in out
    assert out["notFound"] is False
    assert [c["index"] for c in out["citations"]] == [1, 2]
    assert out["citations"][0]["title"] == "ML"
    assert out["citations"][0]["chunkId"] == "c1"
    assert out["bestScore"] >= 0.30


def test_embed_query_includes_previous_user_question() -> None:
    hist = [
        {"role": "user", "content": "What is Ada?"},
        {"role": "assistant", "content": "A person [1]."},
        {"role": "user", "content": "And Meridian?"},
    ]
    assert embed_query_text("what about the second one?", hist) == (
        "And Meridian?\nwhat about the second one?"
    )


def test_vector_search_uses_embed_query(mock_embed: None, monkeypatch: pytest.MonkeyPatch) -> None:
    import query.chat_pipeline as cp

    seen: list[str] = []

    def fake_topk(sqlite_path, embed_model, query, k, document_ids=None):
        seen.append(query)
        return _fake_chunks()

    monkeypatch.setattr(cp, "vector_top_k", fake_topk)
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    d = tempfile.mkdtemp()
    cp.prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        "follow up",
        "nomic-embed-text",
        history=[{"role": "user", "content": "original ask"}],
    )
    assert seen == ["original ask\nfollow up"]
    assert prepare_history([{"role": "assistant", "content": "Hi [3]."}])[0]["content"] == "Hi ."


def test_user_scope_ids_win(mock_embed: None, monkeypatch: pytest.MonkeyPatch) -> None:
    import query.chat_pipeline as cp

    seen_scope: list = []

    def fake_topk(sqlite_path, embed_model, query, k, document_ids=None):
        seen_scope.append(document_ids)
        return _fake_chunks()

    monkeypatch.setattr(cp, "vector_top_k", fake_topk)
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    d = tempfile.mkdtemp()
    out = cp.prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        "Equation (2) from Kipf",
        "nomic-embed-text",
        scope_ids=["user-doc"],
    )
    assert seen_scope == [["user-doc"]]
    assert out["scopeIds"] == ["user-doc"]
    assert out["scopeChosen"] is False
    assert out["scopeTitle"] is None
