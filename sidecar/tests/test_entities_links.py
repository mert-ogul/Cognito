"""Entity extraction merges, labels, and document links."""

from __future__ import annotations

import sqlite3
import tempfile
from pathlib import Path

import numpy as np
import pytest

from common.sqlite_vec_util import MOCK_EMBED_ENV, connect_with_vec, ensure_chunk_embedding_table
from common.text_norm import normalize_date, normalize_name
from ingestion.rpc_entities import (
    find_mentions_links,
    merge_within_doc,
    rpc_recompute_links,
    similar_links,
)
from query.entity_labels import BASE, BY_TYPE, labels_for_document

_MOCK_MODELS = "COGNITO_MOCK_MODELS"

_ENTITIES_DDL = """
CREATE TABLE IF NOT EXISTS entities (
  id TEXT PRIMARY KEY NOT NULL,
  label TEXT NOT NULL,
  name TEXT NOT NULL,
  normalized TEXT NOT NULL,
  doc_count INTEGER NOT NULL DEFAULT 0,
  mention_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS entity_aliases (
  entity_id TEXT NOT NULL,
  alias TEXT NOT NULL,
  UNIQUE(entity_id, alias)
);
CREATE TABLE IF NOT EXISTS mentions (
  entity_id TEXT NOT NULL,
  chunk_id TEXT NOT NULL,
  document_id TEXT NOT NULL,
  start INTEGER NOT NULL,
  end INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS document_links (
  source_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  weight REAL NOT NULL,
  evidence_chunk_id TEXT
);
"""


@pytest.fixture
def mock_models(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(_MOCK_MODELS, "1")
    monkeypatch.setenv(MOCK_EMBED_ENV, "1")


def test_labels_for_document_per_type() -> None:
    paper = labels_for_document("paper", [])
    assert paper[: len(BASE)] == BASE
    assert "dataset" in paper
    assert "method" in paper
    assert "model" in paper
    assert "metric" in paper
    assert "research field" in paper
    assert "task" not in paper
    contract = labels_for_document("contract", ["custom"])
    assert "obligation" in contract
    assert "deadline" in contract
    assert "product or service" in contract
    assert "party" not in contract
    assert "custom" in contract
    letter = labels_for_document("letter", [])
    assert "school" in letter
    assert "job title" in letter
    invoice = labels_for_document("invoice", [])
    assert "product or service" in invoice
    report = labels_for_document("report", [])
    assert "topic" in report
    assert "product" in report
    assert labels_for_document(None, []) == BASE
    for card_type in BY_TYPE:
        out = labels_for_document(card_type, [])
        assert out[: len(BASE)] == BASE


def test_cora_dataset_merge() -> None:
    spans = [
        {
            "label": "dataset",
            "text": "Cora",
            "start": 0,
            "end": 4,
            "score": 0.9,
            "chunk_id": "c1",
            "document_id": "d1",
        },
        {
            "label": "dataset",
            "text": "Cora dataset",
            "start": 10,
            "end": 22,
            "score": 0.9,
            "chunk_id": "c1",
            "document_id": "d1",
        },
    ]
    merged = merge_within_doc(spans)
    norms = {m["normalized"] for m in merged}
    assert norms == {"cora dataset"}
    assert all(m["name"].lower().startswith("cora") for m in merged)


def test_gcn_abbreviation_merge() -> None:
    spans = [
        {
            "label": "model",
            "text": "Graph Convolutional Networks",
            "start": 0,
            "end": 28,
            "score": 0.9,
            "chunk_id": "c1",
            "document_id": "d1",
        },
        {
            "label": "model",
            "text": "GCN",
            "start": 30,
            "end": 33,
            "score": 0.9,
            "chunk_id": "c1",
            "document_id": "d1",
        },
    ]
    context = "We introduce Graph Convolutional Networks (GCN) for graphs."
    merged = merge_within_doc(spans, context_text=context)
    norms = {m["normalized"] for m in merged}
    assert norms == {"graph convolutional networks"}


def test_date_normalisation() -> None:
    assert normalize_date("2020-01-15") == "2020-01-15"
    assert normalize_date("2020-01") == "2020-01"
    assert normalize_date("2020") == "2020"
    assert normalize_date("January 15, 2020") == "2020-01-15"
    assert normalize_date("15 January 2020") == "2020-01-15"
    assert normalize_date("Jan 2020") == "2020-01"
    assert normalize_date("not a date") is None


def test_merge_keeps_year_from_unparsed_date() -> None:
    spans = [
        {
            "label": "date",
            "text": "21 Ekim 1999",
            "start": 0,
            "end": 12,
            "score": 0.9,
        }
    ]
    merged = merge_within_doc(spans)
    assert len(merged) == 1
    assert merged[0]["normalized"] == "1999"
    assert merged[0]["name"] == "1999"
    assert normalize_name("  The Cora  ") == "cora"
    assert normalize_name('"GCN"') == "gcn"


def test_mentions_link_from_title_in_references(mock_models: None) -> None:
    docs = [
        {
            "id": "citing",
            "title": "Later Work",
            "aliases": [],
            "chunks": [
                {
                    "id": "c_ref",
                    "text": (
                        "References\n"
                        "Kipf & Welling. Semi-Supervised Classification with "
                        "Graph Convolutional Networks. ICLR 2017."
                    ),
                }
            ],
        },
        {
            "id": "gcn",
            "title": "Semi-Supervised Classification with Graph Convolutional Networks",
            "aliases": ["GCN"],
            "chunks": [{"id": "c_body", "text": "We propose GCN."}],
        },
    ]
    links = find_mentions_links(docs)
    pairs = {(s, t) for s, t, _e, _w in links}
    assert ("citing", "gcn") in pairs
    evid = {t: e for s, t, e, _w in links if s == "citing" and t == "gcn"}
    assert evid.get("gcn") == "c_ref"


def test_similar_links_threshold() -> None:
    ids = ["a", "b", "c"]
    # a≈b high; c orthogonal-ish
    vecs = np.array(
        [
            [1.0, 0.0, 0.0],
            [0.99, 0.1, 0.0],
            [0.0, 0.0, 1.0],
        ],
        dtype=np.float64,
    )
    links = similar_links(ids, vecs, top_k=3, min_cosine=0.75)
    pair_set = {(s, t) for s, t, _w in links}
    assert ("a", "b") in pair_set
    assert ("b", "a") in pair_set
    assert ("a", "c") not in pair_set
    assert ("c", "a") not in pair_set


def test_recompute_links_mock_writes_rows(mock_models: None) -> None:
    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    con = sqlite3.connect(sqlite_path)
    con.executescript(
        """
        CREATE TABLE documents (
          id TEXT PRIMARY KEY, path TEXT, title TEXT, status TEXT
        );
        CREATE TABLE chunks (
          id TEXT PRIMARY KEY, document_id TEXT, ordinal INT, text TEXT, role TEXT
        );
        CREATE TABLE document_cards (
          document_id TEXT PRIMARY KEY, type TEXT, title TEXT,
          aliases TEXT, summary TEXT
        );
        """
        + _ENTITIES_DDL
    )
    con.execute(
        "INSERT INTO documents VALUES ('d1','/a.pdf',"
        "'Semi-Supervised Classification with Graph Convolutional Networks','indexed')"
    )
    con.execute(
        "INSERT INTO documents VALUES ('d2','/b.pdf','Later Survey Paper About Graphs','indexed')"
    )
    con.execute("INSERT INTO chunks VALUES ('c1','d1',0,'Body of GCN paper.','body')")
    con.execute(
        "INSERT INTO chunks VALUES ('c2','d2',0,"
        "'See Semi-Supervised Classification with Graph Convolutional Networks.','references')"
    )
    con.execute("INSERT INTO document_cards VALUES ('d1','paper','GCN paper','[\"GCN\"]','[]')")
    con.execute("INSERT INTO document_cards VALUES ('d2','paper','Survey','[]','[]')")
    con.commit()
    con.close()

    # Add mock embeddings for similar path (mock mode uses fake similar, but table may be used).
    vcon = connect_with_vec(sqlite_path)
    ensure_chunk_embedding_table(vcon, 4)
    import sqlite_vec

    for cid, vec in (("c1", [1.0, 0.0, 0.0, 0.0]), ("c2", [0.9, 0.1, 0.0, 0.0])):
        blob = sqlite_vec.serialize_float32(vec)
        vcon.execute(
            "INSERT INTO chunk_embeddings(embedding, chunk_id) VALUES (?, ?)",
            (blob, cid),
        )
    vcon.commit()
    vcon.close()

    out = rpc_recompute_links(
        {
            "sqlitePath": sqlite_path,
            "app_data": d,
            "embed_model": "qwen3-embedding:0.6b",
            "background_model": "qwen3.5:4b",
        }
    )
    assert out["ok"] is True
    assert out["mentionsLinks"] >= 1

    con = sqlite3.connect(sqlite_path)
    mention_rows = con.execute(
        "SELECT source_id, target_id FROM document_links WHERE kind = 'mentions'"
    ).fetchall()
    assert ("d2", "d1") in {(r[0], r[1]) for r in mention_rows}
    similar_rows = con.execute(
        "SELECT source_id, target_id FROM document_links WHERE kind = 'similar'"
    ).fetchall()
    assert len(similar_rows) >= 1
    con.close()


def test_windows_stay_under_gliner_token_cap() -> None:
    from ingestion.rpc_entities import _windows, gliner_token_count

    words = [f"word{i}" for i in range(1200)]
    text = " ".join(words)
    windows = _windows(text)
    assert windows
    for _start, _end, window in windows:
        assert gliner_token_count(window) <= 384


def test_incremental_extract_matches_single_pass(mock_models: None) -> None:
    from ingestion.rpc_entities import rpc_extract_entities

    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    con = sqlite3.connect(sqlite_path)
    con.executescript(
        """
        CREATE TABLE documents (
          id TEXT PRIMARY KEY, path TEXT, title TEXT, status TEXT
        );
        CREATE TABLE chunks (
          id TEXT PRIMARY KEY, document_id TEXT, ordinal INT, text TEXT, role TEXT
        );
        CREATE TABLE document_cards (
          document_id TEXT PRIMARY KEY, type TEXT, title TEXT,
          aliases TEXT, summary TEXT
        );
        """
        + _ENTITIES_DDL
    )
    con.execute("INSERT INTO documents VALUES ('d1','/a.pdf','Cora paper','indexed')")
    texts = [
        "The Cora dataset is used for citation classification in 2017.",
        "Graph Convolutional Networks (GCN) beat baselines on Cora.",
        "Later work cites GCN and the Cora dataset again.",
        "Ada Chen published follow-ups in Cambridge during 2018.",
        "Meridian Labs funded the Cora study.",
    ]
    for i, body in enumerate(texts):
        con.execute(
            "INSERT INTO chunks VALUES (?, 'd1', ?, ?, 'body')",
            (f"c{i}", i, body),
        )
    con.execute("INSERT INTO document_cards VALUES ('d1','paper','Cora paper','[]','[]')")
    con.commit()
    con.close()

    params_base = {
        "sqlitePath": sqlite_path,
        "documentId": "d1",
        "app_data": d,
        "labels": [],
    }
    full = rpc_extract_entities({**params_base, "chunkOffset": 0, "chunkLimit": 100})
    assert full["done"] is True
    con = sqlite3.connect(sqlite_path)
    full_mentions = con.execute(
        "SELECT entity_id, chunk_id, start, end FROM mentions ORDER BY chunk_id, start, end"
    ).fetchall()
    con.execute("DELETE FROM mentions")
    con.execute("DELETE FROM entity_aliases")
    con.execute("DELETE FROM entities")
    con.commit()
    con.close()

    offset = 0
    while True:
        out = rpc_extract_entities({**params_base, "chunkOffset": offset, "chunkLimit": 2})
        if out["done"]:
            break
        offset = int(out["nextOffset"])
    con = sqlite3.connect(sqlite_path)
    split_mentions = con.execute(
        "SELECT entity_id, chunk_id, start, end FROM mentions ORDER BY chunk_id, start, end"
    ).fetchall()
    con.close()
    assert split_mentions == full_mentions
    assert len(full_mentions) >= 1


def test_references_without_card_uses_paper_labels(mock_models: None, monkeypatch) -> None:
    from ingestion import rpc_entities as re
    from query.entity_labels import labels_for_document

    seen: list[list[str]] = []

    def capture_batch(app_data, texts, labels, threshold=0.5):
        seen.append(list(labels))
        return [[] for _ in texts]

    monkeypatch.setattr(re, "extract_spans_batch", capture_batch)

    d = tempfile.mkdtemp()
    sqlite_path = str(Path(d) / "v.sqlite")
    con = sqlite3.connect(sqlite_path)
    con.executescript(
        """
        CREATE TABLE documents (
          id TEXT PRIMARY KEY, path TEXT, title TEXT, status TEXT
        );
        CREATE TABLE chunks (
          id TEXT PRIMARY KEY, document_id TEXT, ordinal INT, text TEXT, role TEXT
        );
        CREATE TABLE document_cards (
          document_id TEXT PRIMARY KEY, type TEXT, title TEXT,
          aliases TEXT, summary TEXT
        );
        """
        + _ENTITIES_DDL
    )
    con.execute("INSERT INTO documents VALUES ('d1','/a.pdf','Untitled','indexed')")
    con.execute("INSERT INTO chunks VALUES ('c0','d1',0,'Body about the Cora dataset.','body')")
    con.execute("INSERT INTO chunks VALUES ('c1','d1',1,'1. Prior work.','references')")
    con.commit()
    con.close()

    out = re.rpc_extract_entities(
        {
            "sqlitePath": sqlite_path,
            "documentId": "d1",
            "app_data": d,
            "labels": [],
            "chunkOffset": 0,
            "chunkLimit": 40,
        }
    )
    assert out["done"] is True
    assert seen
    assert seen[0] == labels_for_document("paper", [])
