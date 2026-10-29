"""chunk_embeddings dimension helpers and reset_index."""

from __future__ import annotations

import sqlite3
from pathlib import Path

import duckdb
import pytest

from common.duckdb_util import ensure_schema
from common.sqlite_vec_util import (
    connect_with_vec,
    ensure_chunk_embedding_table,
    insert_chunk_embedding,
    table_dimension,
)
from ingestion.rpc_ingest import rpc_reset_index


def test_table_dimension_reads_size(tmp_path: Path) -> None:
    db = tmp_path / "v.sqlite"
    con = connect_with_vec(str(db))
    assert table_dimension(con) is None
    ensure_chunk_embedding_table(con, 32)
    con.commit()
    assert table_dimension(con) == 32
    con.close()


def test_insert_wrong_size_raises(tmp_path: Path) -> None:
    db = tmp_path / "v.sqlite"
    con = connect_with_vec(str(db))
    ensure_chunk_embedding_table(con, 8)
    con.commit()
    with pytest.raises(ValueError, match="embedding has 4 values but the index uses 8"):
        insert_chunk_embedding(con, "c1", [0.0, 0.0, 0.0, 0.0])
    con.close()


def test_reset_index_removes_table_and_graph_rows(tmp_path: Path) -> None:
    sqlite_path = str(tmp_path / "v.sqlite")
    duck_path = str(tmp_path / "g.duckdb")
    con = connect_with_vec(sqlite_path)
    ensure_chunk_embedding_table(con, 4)
    insert_chunk_embedding(con, "c1", [0.1, 0.2, 0.3, 0.4])
    con.commit()
    con.close()

    ensure_schema(duck_path)
    duck = duckdb.connect(duck_path)
    duck.execute(
        """
        INSERT INTO graph_nodes
        (node_id, name, kind, normalized_name, source_count, confidence_avg, metadata_json, created_at, updated_at)
        VALUES ('n1', 'A', 'entity', 'a', 1, 1.0, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        """
    )
    duck.execute(
        """
        INSERT INTO graph_edges
        (edge_id, source_node_id, target_node_id, relation, document_id, chunk_id, confidence, created_at)
        VALUES ('e1', 'n1', 'n1', 'relates_to', 'd1', 'c1', 1.0, CURRENT_TIMESTAMP)
        """
    )
    duck.close()

    out = rpc_reset_index({"sqlitePath": sqlite_path, "duckPath": duck_path})
    assert out["ok"] is True

    con2 = sqlite3.connect(sqlite_path)
    n = con2.execute(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'chunk_embeddings'"
    ).fetchone()[0]
    con2.close()
    assert n == 0

    duck2 = duckdb.connect(duck_path)
    assert duck2.execute("SELECT COUNT(*) FROM graph_nodes").fetchone()[0] == 0
    assert duck2.execute("SELECT COUNT(*) FROM graph_edges").fetchone()[0] == 0
    duck2.close()
