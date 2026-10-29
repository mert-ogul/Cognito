"""prepare_chat_retrieval: numbered evidence, graph seeds, history."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import duckdb
import pytest

from common.duckdb_util import ensure_schema
from common.sqlite_vec_util import MOCK_EMBED_ENV
from query.chat_pipeline import embed_query_text, prepare_chat_retrieval, prepare_history


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
        },
        {
            "chunk_id": "c2",
            "document_id": "d1",
            "text": "Neural nets are related.",
            "page_start": 2,
            "page_end": 2,
            "title": "ML",
            "path": "/ml.md",
        },
    ]


def test_evidence_numbered_and_citations_carry_index(
    mock_embed: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    import query.chat_pipeline as cp

    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: _fake_chunks())
    d = tempfile.mkdtemp()
    out = cp.prepare_chat_retrieval(str(Path(d) / "v.sqlite"), None, "what is ml", "nomic-embed-text")
    user = out["messages"][-1]["content"]
    assert "[1]" in user and "[2]" in user
    assert [c["index"] for c in out["citations"]] == [1, 2]


def test_seeds_from_retrieved_chunk_edges_and_named_edges(
    mock_embed: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    import query.chat_pipeline as cp

    d = tempfile.mkdtemp()
    duck = str(Path(d) / "g.duckdb")
    ensure_schema(duck)
    con = duckdb.connect(duck)
    con.execute(
        """
        INSERT INTO graph_nodes
        (node_id, name, kind, normalized_name, source_count, confidence_avg, metadata_json, created_at, updated_at)
        VALUES
        ('n1', 'Machine Learning', 'concept', 'machine learning', 1, 1.0, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
        ('n2', 'Neural Net', 'concept', 'neural net', 1, 1.0, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        """
    )
    con.execute(
        """
        INSERT INTO graph_edges
        (edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence, created_at)
        VALUES ('e1', 'n1', 'n2', 'related_to', 'd1', 'c1', 1.0, CURRENT_TIMESTAMP)
        """
    )
    con.close()

    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: _fake_chunks())
    out = cp.prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        duck,
        "Tell me about machine learning",
        "nomic-embed-text",
    )
    assert out["retrievalMode"] == "hybrid"
    user = out["messages"][-1]["content"]
    assert "Machine Learning" in user
    assert "Neural Net" in user
    assert "—[related_to]→" in user
    assert "(from [1])" in user
    assert "n1" not in user and "n_3" not in user
    assert "doc:" not in user
    cite1 = next(c for c in out["citations"] if c["index"] == 1)
    assert set(cite1["nodeIds"]) == {"n1", "n2"}
    cite2 = next(c for c in out["citations"] if c["index"] == 2)
    assert cite2["nodeIds"] == []


def test_history_inserted_stripped_and_capped(mock_embed: None, monkeypatch: pytest.MonkeyPatch) -> None:
    import query.chat_pipeline as cp

    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: _fake_chunks())
    d = tempfile.mkdtemp()
    history = [
        {"role": "user", "content": "first question"},
        {"role": "assistant", "content": "Answer with [1] and [2] markers."},
        {"role": "user", "content": "x" * 3000},
        {"role": "assistant", "content": "y" * 3000},
        {"role": "user", "content": "z" * 3000},
    ]
    out = cp.prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        None,
        "current",
        "nomic-embed-text",
        history=history,
    )
    msgs = out["messages"]
    assert msgs[0]["role"] == "system"
    assert msgs[-1]["role"] == "user"
    assert "Question:\ncurrent" in msgs[-1]["content"]
    mid = msgs[1:-1]
    assert mid
    for m in mid:
        if m["role"] == "assistant":
            assert "[1]" not in m["content"] and "[2]" not in m["content"]
        assert len(m["content"]) <= 2000
    assert sum(len(m["content"]) for m in mid) <= 6000
    # Oldest turns dropped first when over cap.
    assert not any(m["content"] == "first question" for m in mid)


def test_embed_query_includes_previous_user_question() -> None:
    hist = [
        {"role": "user", "content": "What is Ada?"},
        {"role": "assistant", "content": "A person [1]."},
        {"role": "user", "content": "And Meridian?"},
    ]
    assert embed_query_text("what about the second one?", hist) == (
        "And Meridian?\nwhat about the second one?"
    )


def test_vector_search_uses_embed_query(
    mock_embed: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    import query.chat_pipeline as cp

    seen: list[str] = []

    def fake_topk(sqlite_path, embed_model, query, k):
        seen.append(query)
        return _fake_chunks()

    monkeypatch.setattr(cp, "vector_top_k", fake_topk)
    d = tempfile.mkdtemp()
    cp.prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        None,
        "follow up",
        "nomic-embed-text",
        history=[{"role": "user", "content": "original ask"}],
    )
    assert seen == ["original ask\nfollow up"]
    assert prepare_history([{"role": "assistant", "content": "Hi [3]."}])[0]["content"] == "Hi ."
