"""rpc_store_graph_chunk writes nodes/edges and is idempotent."""

from __future__ import annotations

import tempfile
from pathlib import Path

from ingestion.rpc_graph import rpc_store_graph_chunk
from extraction.graph_store import open_connection


def test_store_graph_chunk_writes_and_is_idempotent() -> None:
    d = tempfile.mkdtemp()
    duck = str(Path(d) / "g.duckdb")
    params = {
        "duckPath": duck,
        "documentId": "d1",
        "chunkId": "c1",
        "entities": [
            {"name": "Ada Chen", "kind": "person"},
            {"name": "Meridian Labs", "kind": "organization"},
        ],
        "relations": [
            {"source": "Ada Chen", "target": "Meridian Labs", "relation": "works at"},
        ],
    }
    r1 = rpc_store_graph_chunk(params)
    assert r1["ok"] is True
    assert r1["entities"] == 2
    assert r1["edges"] == 1

    r2 = rpc_store_graph_chunk(params)
    assert r2["ok"] is True

    con = open_connection(duck)
    try:
        n = int(con.execute("SELECT COUNT(*) FROM graph_nodes").fetchone()[0])
        e = int(con.execute("SELECT COUNT(*) FROM graph_edges").fetchone()[0])
        assert n == 2
        assert e == 1
        row = con.execute(
            "SELECT relation FROM graph_edges WHERE chunk_id = 'c1'"
        ).fetchone()
        assert row[0] == "works at"
    finally:
        con.close()
