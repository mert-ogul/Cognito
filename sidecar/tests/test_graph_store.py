"""DuckDB graph merge and purge (no LLM)."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

from extraction.graph_store import (
    edge_id_deterministic,
    node_id_from_normalized,
    open_connection,
    purge_document_graph,
    upsert_entities_edges,
)
from extraction.extraction_result import ExtractedEdge, ExtractedEntity


def test_node_and_edge_upsert_idempotent():
    d = tempfile.mkdtemp()
    p = str(Path(d) / "x.duckdb")
    con = open_connection(p)
    try:
        doc, chunk = "doc-a", "chunk-1"
        ents = [
            ExtractedEntity("Acme Corp", "organization", "acme corp", 0.9),
        ]
        edges = [
            ExtractedEdge("acme corp", "alice", "employs", 0.85),
        ]
        upsert_entities_edges(con, doc, chunk, ents, edges)
        con.commit()
        n = con.execute(
            "SELECT COUNT(*) FROM graph_nodes WHERE normalized_name = 'acme corp'"
        ).fetchone()[0]
        assert int(n) >= 1
        eid = edge_id_deterministic(doc, chunk, "acme corp", "alice", "employs")
        assert eid.startswith("e_")
        upsert_entities_edges(con, doc, chunk, ents, edges)
        con.commit()
        n2 = con.execute("SELECT COUNT(*) FROM graph_edges WHERE edge_id = ?", [eid]).fetchone()[0]
        assert int(n2) == 1
    finally:
        con.close()


def test_purge_document_removes_edges_and_trims_evidence():
    d = tempfile.mkdtemp()
    p = str(Path(d) / "y.duckdb")
    con = open_connection(p)
    try:
        upsert_entities_edges(
            con,
            "doc-x",
            "c1",
            [ExtractedEntity("Zed", "concept", "zed", 0.8)],
            [],
        )
        con.commit()
    finally:
        con.close()
    purge_document_graph(p, "doc-x")
    con = open_connection(p)
    try:
        n = con.execute("SELECT COUNT(*) FROM graph_nodes WHERE normalized_name = 'zed'").fetchone()[0]
        assert int(n) == 0
    finally:
        con.close()
