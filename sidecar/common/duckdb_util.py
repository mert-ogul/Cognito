"""DuckDB file initialization for the graph store."""

from __future__ import annotations

from pathlib import Path

import duckdb

_INIT = Path(__file__).resolve().parent.parent / "migrations" / "duckdb_init.sql"


def _migrate_legacy_nullable_edges(con: duckdb.DuckDBPyConnection) -> None:
    """Upgrade older DBs where graph_edges allowed NULL document_id/chunk_id."""
    try:
        con.execute(
            "DELETE FROM graph_edges WHERE document_id IS NULL OR chunk_id IS NULL"
        )
        con.execute("ALTER TABLE graph_edges ALTER COLUMN document_id SET NOT NULL")
        con.execute("ALTER TABLE graph_edges ALTER COLUMN chunk_id SET NOT NULL")
    except Exception:
        pass


def ensure_schema(duck_path: str) -> None:
    sql = _INIT.read_text(encoding="utf-8")
    con = duckdb.connect(duck_path)
    try:
        con.execute(sql)
        _migrate_legacy_nullable_edges(con)
    finally:
        con.close()
