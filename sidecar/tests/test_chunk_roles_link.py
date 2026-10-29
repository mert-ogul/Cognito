"""Chunk role detection, link_document, and name normalizer."""

from __future__ import annotations

import tempfile
from pathlib import Path

from extraction.node_normalizer import normalize_name
from ingestion.chunking import assign_chunk_role, citation_line_ratio, is_references_heading
from ingestion.rpc_link_document import rpc_link_document
from extraction.graph_store import open_connection, upsert_entities_edges
from extraction.extraction_result import ExtractedEntity


def test_references_heading_small_caps() -> None:
    assert is_references_heading("R EFERENCES")
    assert is_references_heading("# References")
    assert is_references_heading("Bibliography")
    assert is_references_heading("ACKNOWLEDGEMENTS")
    assert not is_references_heading("Related work")


def test_citation_lines_mark_references_role() -> None:
    refs = "\n".join(
        [
            "13. S. Milgram, Psychol. Today 2, 60 (1967).",
            "14. A. Author, Nature 1, 2 (2001).",
            "15. B. Writer, Science 3, 4 (1999).",
            "Some leftover note without a year here.",
        ]
    )
    assert citation_line_ratio(refs) >= 0.4
    assert (
        assign_chunk_role(refs, [], after_references_heading=False) == "references"
    )


def test_ordinary_paragraph_with_year_stays_body() -> None:
    body = (
        "In 2017, graph attention networks were proposed as an alternative "
        "to spectral methods. The approach operates on graph-structured data."
    )
    assert citation_line_ratio(body) < 0.4
    assert assign_chunk_role(body, [], after_references_heading=False) == "body"


def test_after_references_heading_forces_role() -> None:
    assert (
        assign_chunk_role("anything", [], after_references_heading=True) == "references"
    )


def test_normalize_strips_quotes_punct_and_the() -> None:
    assert normalize_name('  "Graph Attention Networks". ') == "graph attention networks"
    assert normalize_name("The GCN model") == "gcn model"
    assert normalize_name("(Ada)") == "ada"


def test_link_document_work_node_and_multi_chunk_edges() -> None:
    d = tempfile.mkdtemp()
    duck = str(Path(d) / "g.duckdb")
    con = open_connection(duck)
    try:
        upsert_entities_edges(
            con,
            "doc1",
            "c1",
            [
                ExtractedEntity("Graph attention network", "method", "graph attention network", 0.8),
                ExtractedEntity("Cora", "dataset", "cora", 0.8),
            ],
            [],
        )
        upsert_entities_edges(
            con,
            "doc1",
            "c2",
            [
                ExtractedEntity("Graph attention network", "method", "graph attention network", 0.8),
                ExtractedEntity("Pubmed", "dataset", "pubmed", 0.8),
            ],
            [],
        )
        con.commit()
    finally:
        con.close()

    out = rpc_link_document(
        {
            "duckPath": duck,
            "documentId": "doc1",
            "title": "Graph Attention Networks",
            "firstChunkId": "c1",
        }
    )
    assert out["ok"] is True
    assert out["linked"] == 1

    con = open_connection(duck)
    try:
        work = con.execute(
            "SELECT kind, name FROM graph_nodes WHERE kind = 'work'"
        ).fetchall()
        assert len(work) == 1
        assert work[0][1] == "Graph Attention Networks"
        edges = con.execute(
            "SELECT relation, source_node_id, target_node_id FROM graph_edges WHERE relation = 'discusses'"
        ).fetchall()
        assert len(edges) == 1
        assert edges[0][0] == "discusses"
    finally:
        con.close()
