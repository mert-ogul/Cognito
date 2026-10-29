"""Title cleaning, embed prefixes, FTS query builder, and RRF fusion."""

from __future__ import annotations

from common.ollama import embed_prefixes
from ingestion.parsing import clean_title, _metadata_title_ok
from query.chat_pipeline import build_fts_query, reciprocal_rank_fusion


def test_clean_title_joins_small_caps_and_dashes() -> None:
    assert (
        clean_title("S EMI -S UPERVISED CLASSIFICATION WITH GRAPH CONVOLUTIONAL NETWORKS")
        == "Semi-Supervised Classification with Graph Convolutional Networks"
    )


def test_clean_title_all_capitals_to_title_case() -> None:
    assert clean_title("GRAPH ATTENTION NETWORKS") == "Graph Attention Networks"
    assert clean_title("THE BARABASI ALBERT MODEL") == "The Barabasi Albert Model"


def test_metadata_title_rejects_arxiv_and_empty() -> None:
    assert not _metadata_title_ok("")
    assert not _metadata_title_ok("arXiv:1609.02907")
    assert not _metadata_title_ok("Microsoft Word - draft")
    assert not _metadata_title_ok("notes.doc")
    assert not _metadata_title_ok("Too Short")
    assert _metadata_title_ok("Semi-Supervised Classification with Graph Convolutional Networks")


def test_embed_prefixes_by_model() -> None:
    assert embed_prefixes("nomic-embed-text") == ("search_query: ", "search_document: ")
    assert embed_prefixes("nomic-embed-text:latest") == ("search_query: ", "search_document: ")
    assert embed_prefixes("mxbai-embed-large") == (
        "Represent this sentence for searching relevant passages: ",
        "",
    )
    assert embed_prefixes("other-model") == ("", "")


def test_build_fts_query_apostrophes_equation_empty() -> None:
    assert build_fts_query("") == ""
    assert build_fts_query("   ") == ""
    assert build_fts_query("Equation (2) from Kipf") == '"Equation" OR "2" OR "Kipf"'
    assert build_fts_query("what's GCN") == '"what\'s" OR "GCN"'
    assert build_fts_query('say "hi" now') == '"say" OR "hi" OR "now"'


def test_reciprocal_rank_fusion_order() -> None:
    vector = [
        {"chunk_id": "a", "text": "va"},
        {"chunk_id": "b", "text": "vb"},
        {"chunk_id": "c", "text": "vc"},
    ]
    keyword = [
        {"chunk_id": "c", "text": "kc"},
        {"chunk_id": "a", "text": "ka"},
        {"chunk_id": "d", "text": "kd"},
    ]
    fused = reciprocal_rank_fusion([vector, keyword])
    assert [c["chunk_id"] for c in fused] == ["a", "c", "b", "d"]


def test_prompt_labels_use_title(monkeypatch) -> None:
    import tempfile
    from pathlib import Path

    import query.chat_pipeline as cp
    from common.sqlite_vec_util import MOCK_EMBED_ENV

    monkeypatch.setenv(MOCK_EMBED_ENV, "1")
    monkeypatch.setattr(
        cp,
        "vector_top_k",
        lambda *a, **k: [
            {
                "chunk_id": "c1",
                "document_id": "d1",
                "text": "renormalization trick",
                "page_start": 2,
                "page_end": 2,
                "title": "Semi-Supervised Classification with Graph Convolutional Networks",
                "path": "/vault/1609.02907v4.pdf",
            }
        ],
    )
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])

    d = tempfile.mkdtemp()
    out = cp.prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        None,
        "Equation (2)",
        "nomic-embed-text",
    )
    user = out["messages"][-1]["content"]
    assert (
        "[1] Semi-Supervised Classification with Graph Convolutional Networks, p. 2" in user
    )
    assert "1609.02907" not in user
    assert "LaTeX" in out["messages"][0]["content"]
