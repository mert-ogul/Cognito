"""Fixture-style test: mock LightRAG per-chunk output → DuckDB."""

from __future__ import annotations

import asyncio
import sqlite3
import tempfile
from pathlib import Path
from unittest.mock import patch

from extraction.extraction_service import extract_document_graph_async
from extraction.graph_store import open_connection


def test_mocked_lightrag_merge_writes_nodes_and_edges():
    fake_nodes = {
        "Acme": [
            {
                "entity_name": "Acme",
                "entity_type": "organization",
                "description": "A company",
            }
        ]
    }
    fake_edges = {
        ("Acme", "Bob"): [
            {
                "src_id": "Acme",
                "tgt_id": "Bob",
                "weight": 0.9,
                "description": "employs",
                "keywords": "work",
            }
        ]
    }

    async def fake_extract(_document_id, _path, _chunks, _cfg):
        return [("c1", fake_nodes, fake_edges)]

    d = tempfile.mkdtemp()
    duck = str(Path(d) / "t.duckdb")
    sqlite = str(Path(d) / "v.sqlite")
    sq = sqlite3.connect(sqlite)
    sq.executescript(
        """
        CREATE TABLE documents (id TEXT, path TEXT);
        CREATE TABLE chunks (id TEXT, document_id TEXT, ordinal INT, text TEXT);
        INSERT INTO documents VALUES ('d1', '/x.md');
        INSERT INTO chunks VALUES ('c1', 'd1', 0, 'hello');
        """
    )
    sq.commit()
    sq.close()

    with patch(
        "extraction.extraction_service.extract_chunks_sequential",
        side_effect=fake_extract,
    ):
        r = asyncio.run(extract_document_graph_async(sqlite, duck, "d1", "phi4-mini"))
    assert r.get("ok") is True
    con = open_connection(duck)
    try:
        n = int(con.execute("SELECT COUNT(*) FROM graph_nodes").fetchone()[0])
        e = int(con.execute("SELECT COUNT(*) FROM graph_edges").fetchone()[0])
        assert n >= 1
        assert e >= 1
    finally:
        con.close()
