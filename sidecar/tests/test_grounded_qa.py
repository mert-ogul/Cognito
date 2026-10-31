"""Scope resolution, quote validation, claim checks, low-score notFound."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import pytest

from common.sqlite_vec_util import MOCK_EMBED_ENV
from query.chat_pipeline import (
    author_lines_from_front,
    prepare_chat_retrieval,
    rebuild_document_profile,
    resolve_scope,
)
from query.check_answer import (
    check_answer,
    claims_supported,
    looks_like_formula,
    normalize_evidence_text,
    quote_in_passage,
)

_DOCUMENTS_FTS_DDL = """
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  title TEXT,
  path TEXT,
  status TEXT
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
  regions TEXT
);
CREATE TABLE document_cards (
  document_id TEXT PRIMARY KEY,
  people TEXT,
  organizations TEXT,
  aliases TEXT
);
CREATE TABLE documents_profile (
  document_id TEXT PRIMARY KEY,
  profile TEXT NOT NULL
);
CREATE VIRTUAL TABLE documents_fts USING fts5(
  profile,
  content='documents_profile',
  content_rowid='rowid',
  tokenize='porter unicode61 remove_diacritics 2'
);
CREATE TRIGGER documents_profile_ai AFTER INSERT ON documents_profile BEGIN
  INSERT INTO documents_fts(rowid, profile) VALUES (new.rowid, new.profile);
END;
CREATE TRIGGER documents_profile_ad AFTER DELETE ON documents_profile BEGIN
  INSERT INTO documents_fts(documents_fts, rowid, profile) VALUES('delete', old.rowid, old.profile);
END;
CREATE TRIGGER documents_profile_au AFTER UPDATE ON documents_profile BEGIN
  INSERT INTO documents_fts(documents_fts, rowid, profile) VALUES('delete', old.rowid, old.profile);
  INSERT INTO documents_fts(rowid, profile) VALUES (new.rowid, new.profile);
END;
"""

_GCN_PAGE1 = (
    "Semi-Supervised Classification with Graph Convolutional Networks\n"
    "Thomas N. Kipf\n"
    "Max Welling\n"
    "University of Amsterdam\n"
    "\n"
    "Abstract\n"
    "We present a scalable approach for semi-supervised learning on graph-structured data "
    "that is based on an efficient variant of convolutional neural networks which operate "
    "directly on graphs. We motivate the choice of our convolutional architecture via a "
    "localized first-order approximation of spectral graph convolutions.\n"
)

_GAT_PAGE1 = (
    "Graph Attention Networks\n"
    "Petar Velickovic\n"
    "Guillem Cucurull\n"
    "Arantxa Casanova\n"
    "Adriana Romero\n"
    "Pietro Lio\n"
    "Yoshua Bengio\n"
    "\n"
    "Abstract\n"
    "We present graph attention networks (GATs), novel neural network architectures that "
    "operate on graph-structured data, leveraging masked self-attentional layers. "
    "In contrast to related work such as Kipf & Welling (2017), our model assigns "
    "different importance to different nodes.\n"
)

_GCN_TABLE2 = (
    "Table 2: Summary of results in terms of classification accuracy (in percent).\n"
    "Method Cora Citeseer Pubmed\n"
    "GCN 81.5 70.3 79.0\n"
    "Dataset Cora is a citation network.\n"
)

_GAT_HEADER = (
    "Published as a conference paper at ICLR 2018\n"
    "Graph Attention Networks\n"
    "Petar Veličković\n"
    "Guillem Cucurull\n"
    "Arantxa Casanova\n"
    "Adriana Romero\n"
    "Pietro Liò\n"
    "Yoshua Bengio\n"
)


@pytest.fixture(autouse=True)
def mock_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(MOCK_EMBED_ENV, "1")
    monkeypatch.setenv("COGNITO_MOCK_MODELS", "1")


def _fixture_sqlite() -> str:
    d = tempfile.mkdtemp()
    path = str(Path(d) / "v.sqlite")
    con = sqlite3.connect(path)
    try:
        con.executescript(_DOCUMENTS_FTS_DDL)
        con.execute(
            "INSERT INTO documents VALUES (?,?,?,?)",
            (
                "gcn",
                "Semi-Supervised Classification with Graph Convolutional Networks",
                "/papers/1609.02907v4.pdf",
                "indexed",
            ),
        )
        con.execute(
            "INSERT INTO documents VALUES (?,?,?,?)",
            ("gat", "Graph Attention Networks", "/papers/gat.pdf", "indexed"),
        )
        con.execute(
            "INSERT INTO chunks VALUES (?,?,?,?,?,?,?,?,?)",
            ("c_gcn0", "gcn", 0, _GCN_PAGE1, 1, 1, "body", "[]", None),
        )
        con.execute(
            "INSERT INTO chunks VALUES (?,?,?,?,?,?,?,?,?)",
            ("c_gat0", "gat", 0, _GAT_PAGE1, 1, 1, "body", "[]", None),
        )
        con.execute(
            "INSERT INTO document_cards(document_id, people, organizations, aliases) VALUES (?,?,?,?)",
            (
                "gcn",
                '[{"name":"Thomas N. Kipf"},{"name":"Max Welling"}]',
                '["University of Amsterdam"]',
                '["GCN"]',
            ),
        )
        con.execute(
            "INSERT INTO document_cards(document_id, people, organizations, aliases) VALUES (?,?,?,?)",
            ("gat", "[]", "[]", '["GAT"]'),
        )
        con.commit()
    finally:
        con.close()
    rebuild_document_profile(path, "gcn")
    rebuild_document_profile(path, "gat")
    return path


def test_resolve_scope_picks_gcn_paper() -> None:
    path = _fixture_sqlite()
    q = "Equation (2) from Kipf and Welling's GCN paper"
    out = resolve_scope(path, q)
    assert out["scopeChosen"] is True
    assert out["scopeIds"] == ["gcn"]
    assert len(out["profileMatches"]) >= 1
    assert out["profileMatches"][0]["documentId"] == "gcn"


def test_resolve_scope_explicit_wins() -> None:
    path = _fixture_sqlite()
    out = resolve_scope(path, "Equation (2) from Kipf", explicit_ids=["gat"])
    assert out["scopeIds"] == ["gat"]
    assert out["scopeChosen"] is False
    assert out["scopeReason"] == "explicit"


def test_author_lines_from_front_stops_at_abstract() -> None:
    text = (
        "Semi-Supervised Classification with Graph Convolutional Networks\n"
        "Thomas N. Kipf\n"
        "Max Welling\n"
        "University of Amsterdam\n"
        "\n"
        "Abstract\n"
        "We present a scalable approach...\n"
    )
    lines = author_lines_from_front(
        text, "Semi-Supervised Classification with Graph Convolutional Networks"
    )
    assert "Thomas N. Kipf" in lines
    assert "Max Welling" in lines
    assert not any("Abstract" in ln or "scalable" in ln for ln in lines)


def test_quote_in_passage_exact_and_line_break() -> None:
    passage = "We use the renorm-\nalization trick for training."
    assert quote_in_passage("renormalization trick", passage) is True
    assert quote_in_passage("We use the renormalization trick", passage) is True


def test_quote_in_passage_one_stray_token() -> None:
    passage = "The model reaches 81.5 percent accuracy on Cora."
    assert quote_in_passage("The model reaches 81.5 accuracy on Cora", passage) is True


def test_quote_in_passage_rejects_invented_added_not_swapped_dropped() -> None:
    passage = "The model reaches 81.5 percent accuracy on Cora."
    assert quote_in_passage("invented phrase nowhere present", passage) is False
    assert (
        quote_in_passage("The model does not reach 81.5 percent accuracy on Cora", passage) is False
    )
    assert quote_in_passage("The model reaches accuracy percent 81.5 on Cora", passage) is False
    assert quote_in_passage("The model on Cora", passage) is False


def test_looks_like_formula() -> None:
    assert looks_like_formula(r"$H^{(l+1)}=\sigma(\tilde{A}H^{(l)}W^{(l)})$") is True
    assert looks_like_formula(r"\tilde{A}=D^{-1/2}AD^{-1/2}") is True
    assert looks_like_formula("The model is trained on Cora.") is False
    assert looks_like_formula(r"GCN reaches $81.5\%$ on the Cora dataset.") is False
    assert looks_like_formula(r"$81.5\%$") is True


def test_check_answer_unanswerable_returns_no_sentences() -> None:
    passages = [{"index": 1, "text": "Something about graphs."}]
    draft = {
        "answerable": False,
        "answer": [
            {
                "quotes": [{"source": 1, "text": "Something about graphs."}],
                "sentence": "The model uses graphs.",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["notFound"] is True
    assert out["sentences"] == []


def test_check_answer_keeps_formula_quote() -> None:
    passages = [{"index": 1, "text": "Layer update uses the renormalized adjacency."}]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [
                    {
                        "source": 1,
                        "text": r"$H=\sigma(\tilde{A}HW)$",
                    }
                ],
                "sentence": r"$$H=\sigma(\tilde{A}HW)$$",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["notFound"] is False
    assert out["sentences"][0]["status"] == "unchecked"
    assert out["sentences"][0]["reason"] == "formula"


def test_claims_supported_gcn_table2_accuracy() -> None:
    evidence = [_GCN_TABLE2]
    assert claims_supported(
        "GCN achieves an accuracy of 81.5% on the Cora dataset.",
        evidence,
    )
    assert not claims_supported(
        "GAT achieves an accuracy of 81.5% on the Cora dataset.",
        evidence,
    )
    assert not claims_supported(
        "GCN achieves an accuracy of 85.5% on the Cora dataset.",
        evidence,
    )


def test_claims_supported_gat_header_author_and_venue() -> None:
    evidence = [_GAT_HEADER]
    assert claims_supported(
        "Petar Veličković wrote the paper titled Graph Attention Networks.",
        evidence,
    )
    assert not claims_supported(
        "Thomas Kipf wrote the paper titled Graph Attention Networks.",
        evidence,
    )
    assert claims_supported(
        "The paper was published at ICLR 2018.",
        evidence,
    )
    assert not claims_supported(
        "The paper was published at NeurIPS 2018.",
        evidence,
    )


def test_check_answer_corrects_quote_source_across_passages() -> None:
    passages = [
        {"index": 1, "title": "GCN", "text": "Something unrelated about convolutions."},
        {
            "index": 2,
            "title": "GAT",
            "text": "We present graph attention networks that operate on graphs.",
        },
    ]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [
                    {
                        "source": 1,
                        "text": "graph attention networks that operate on graphs",
                    }
                ],
                "sentence": "The paper presents graph attention networks.",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["notFound"] is False
    assert out["sentences"][0]["status"] in ("supported", "unchecked")
    assert out["sentences"][0]["sources"] == [2]


def test_check_answer_hhem_weak_keeps_answer(monkeypatch: pytest.MonkeyPatch) -> None:
    def fake_bounded(_app, pairs):
        return [0.2] * len(pairs)

    monkeypatch.setattr("query.check_answer.hhem_scores_bounded", fake_bounded)
    passages = [{"index": 1, "title": "Cora", "text": "The model uses Cora."}]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [{"source": 1, "text": "uses Cora"}],
                "sentence": "It uses Cora.",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["notFound"] is False
    assert out["sentences"][0]["status"] == "unchecked"
    assert out["sentences"][0]["reason"] == "weak"


def test_check_answer_two_sentences_each_with_own_quote(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def fake_bounded(_app, pairs):
        return [0.9] * len(pairs)

    monkeypatch.setattr("query.check_answer.hhem_scores_bounded", fake_bounded)
    passages = [
        {
            "index": 1,
            "title": "GCN",
            "text": "GCN achieves 81.5 accuracy on the Cora dataset.",
        },
        {
            "index": 2,
            "title": "GAT",
            "text": "Published as a conference paper at ICLR 2018.",
        },
    ]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [
                    {
                        "source": 1,
                        "text": "GCN achieves 81.5 accuracy on the Cora dataset.",
                    }
                ],
                "sentence": "GCN achieves an accuracy of 81.5 on the Cora dataset.",
            },
            {
                "quotes": [
                    {
                        "source": 2,
                        "text": "Published as a conference paper at ICLR 2018.",
                    }
                ],
                "sentence": "The paper was published at ICLR 2018.",
            },
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["notFound"] is False
    assert len(out["sentences"]) == 2
    assert out["sentences"][0]["status"] == "supported"
    assert out["sentences"][0]["sources"] == [1]
    assert out["sentences"][1]["status"] == "supported"
    assert out["sentences"][1]["sources"] == [2]


def test_name_number_check_catches_year_mismatch() -> None:
    passages = [
        {
            "index": 1,
            "text": "Kipf and Welling published the model in 2016.",
        }
    ]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [
                    {
                        "source": 1,
                        "text": "Kipf and Welling published the model in 2016.",
                    }
                ],
                "sentence": "Kipf and Welling published the model in 2017.",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["notFound"] is True
    assert out["sentences"][0]["status"] == "unsupported"


def test_low_best_score_returns_not_found(monkeypatch: pytest.MonkeyPatch) -> None:
    import query.chat_pipeline as cp

    chunks = [
        {
            "chunk_id": f"c{i}",
            "document_id": "d1",
            "text": f"passage {i} about graphs",
            "page_start": i,
            "page_end": i,
            "title": "ML",
            "path": "/ml.md",
            "role": "body",
            "labels": [],
        }
        for i in range(1, 6)
    ]
    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: chunks)
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    monkeypatch.setattr(
        cp,
        "rerank_with_budget",
        lambda app_data, query, documents: {
            "scores": [0.008] * len(documents),
            "scored": len(documents),
            "budgetHit": False,
            "rerankMs": 1,
        },
    )
    monkeypatch.setattr(
        cp,
        "resolve_scope",
        lambda *a, **k: {
            "scopeIds": [],
            "scopeTitle": None,
            "scopeChosen": False,
            "scopeReason": "no_profile_match",
            "profileMatches": [],
        },
    )
    d = tempfile.mkdtemp()
    out = prepare_chat_retrieval(
        str(Path(d) / "v.sqlite"),
        "what is this",
        "nomic-embed-text",
    )
    assert out["notFound"] is True
    assert out["bestScore"] < 0.015
    assert len(out["citations"]) == 3
    assert [c["index"] for c in out["citations"]] == [1, 2, 3]
    assert "trace" in out
    assert "text" in out["citations"][0]


def test_second_pass_answers_from_scoped_document(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import query.chat_pipeline as cp

    path = _fixture_sqlite()
    gcn_chunks = [
        {
            "chunk_id": "eq2",
            "document_id": "gcn",
            "text": "Equation (2) defines the renormalized adjacency used in the GCN layer.",
            "page_start": 2,
            "page_end": 2,
            "title": "Semi-Supervised Classification with Graph Convolutional Networks",
            "path": "/papers/1609.02907v4.pdf",
            "role": "body",
            "labels": ["(2)"],
        }
    ]
    gat_chunks = [
        {
            "chunk_id": "gat1",
            "document_id": "gat",
            "text": "Attention coefficients are computed for neighbouring nodes.",
            "page_start": 3,
            "page_end": 3,
            "title": "Graph Attention Networks",
            "path": "/papers/gat.pdf",
            "role": "body",
            "labels": [],
        }
    ]

    def fake_vector(sqlite_path, embed_model, query, k, document_ids=None):
        if document_ids == ["gcn"]:
            return list(gcn_chunks)
        if document_ids == ["gat"]:
            return list(gat_chunks)
        return list(gat_chunks)

    scores_by_text = {
        gcn_chunks[0]["text"]: 0.95,
        gat_chunks[0]["text"]: 0.008,
    }

    def fake_rerank(app_data, query, documents):
        scores = [scores_by_text.get(t, 0.2) for t in documents]
        return {
            "scores": scores,
            "scored": len(documents),
            "budgetHit": False,
            "rerankMs": 1,
        }

    monkeypatch.setattr(cp, "vector_top_k", fake_vector)
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    monkeypatch.setattr(cp, "rerank_with_budget", fake_rerank)
    # Force ambiguous first-pass scope so second pass runs.
    monkeypatch.setattr(
        cp,
        "resolve_scope",
        lambda *a, **k: {
            "scopeIds": [],
            "scopeTitle": None,
            "scopeChosen": False,
            "scopeReason": "ambiguous_or_weak",
            "profileMatches": [
                {
                    "documentId": "gcn",
                    "title": "Semi-Supervised Classification with Graph Convolutional Networks",
                    "path": "/papers/1609.02907v4.pdf",
                    "score": 1.0,
                    "termHits": 1,
                },
                {
                    "documentId": "gat",
                    "title": "Graph Attention Networks",
                    "path": "/papers/gat.pdf",
                    "score": 0.9,
                    "termHits": 1,
                },
            ],
        },
    )

    out = prepare_chat_retrieval(
        path,
        "Write Equation (2) from Kipf and Welling's GCN paper",
        "nomic-embed-text",
        app_data="/tmp",
    )
    assert out["notFound"] is False
    assert out["scopeChosen"] is True
    assert out["scopeIds"] == ["gcn"]
    assert out["trace"]["secondPass"] is True
    assert out["citations"][0]["documentId"] == "gcn"


def test_suggestions_when_both_passes_fail(monkeypatch: pytest.MonkeyPatch) -> None:
    import query.chat_pipeline as cp

    path = _fixture_sqlite()
    weak = [
        {
            "chunk_id": "w1",
            "document_id": "gat",
            "text": "unrelated passage about attention",
            "page_start": 1,
            "page_end": 1,
            "title": "Graph Attention Networks",
            "path": "/papers/gat.pdf",
            "role": "body",
            "labels": [],
        }
    ]
    monkeypatch.setattr(cp, "vector_top_k", lambda *a, **k: list(weak))
    monkeypatch.setattr(cp, "keyword_top_k", lambda *a, **k: [])
    monkeypatch.setattr(
        cp,
        "rerank_with_budget",
        lambda app_data, query, documents: {
            "scores": [0.008] * len(documents),
            "scored": len(documents),
            "budgetHit": False,
            "rerankMs": 1,
        },
    )
    monkeypatch.setattr(
        cp,
        "resolve_scope",
        lambda *a, **k: {
            "scopeIds": [],
            "scopeTitle": None,
            "scopeChosen": False,
            "scopeReason": "ambiguous_or_weak",
            "profileMatches": [
                {
                    "documentId": "gcn",
                    "title": "GCN",
                    "path": "/papers/1609.02907v4.pdf",
                    "score": 1.0,
                    "termHits": 1,
                },
                {
                    "documentId": "gat",
                    "title": "GAT",
                    "path": "/papers/gat.pdf",
                    "score": 0.8,
                    "termHits": 1,
                },
            ],
        },
    )
    out = prepare_chat_retrieval(
        path, "something obscure about mars", "nomic-embed-text", app_data="/tmp"
    )
    assert out["notFound"] is True
    assert len(out["suggestions"]) == 2
    assert out["suggestions"][0]["label"].startswith("Search in ")
    assert out["trace"]["secondPass"] is False


def test_normalize_evidence_ligatures_and_quotes() -> None:
    assert "fi" in normalize_evidence_text("ﬁle")
    assert normalize_evidence_text("word-\nbreak") == "wordbreak"
    assert '"' in normalize_evidence_text("“quoted”")
