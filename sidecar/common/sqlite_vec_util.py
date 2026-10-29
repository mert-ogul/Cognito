"""sqlite-vec virtual table helpers for chunk embeddings."""

from __future__ import annotations

import os
import re
import sqlite3

import sqlite_vec

MOCK_EMBED_DIM = 768
MOCK_EMBED_ENV = "COGNITO_MOCK_EMBED"

# Kept for callers that still import the mock size under the old name.
VEC_DIM = MOCK_EMBED_DIM


def connect_with_vec(path: str) -> sqlite3.Connection:
    con = sqlite3.connect(path)
    con.execute("PRAGMA foreign_keys = ON")
    con.enable_load_extension(True)
    sqlite_vec.load(con)
    return con


def table_dimension(con: sqlite3.Connection) -> int | None:
    row = con.execute(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'chunk_embeddings'"
    ).fetchone()
    if not row or not row[0]:
        return None
    m = re.search(r"float\[(\d+)\]", str(row[0]))
    if not m:
        return None
    return int(m.group(1))


def ensure_chunk_embedding_table(con: sqlite3.Connection, dim: int) -> None:
    con.execute(
        f"""
        CREATE VIRTUAL TABLE IF NOT EXISTS chunk_embeddings USING vec0(
          embedding float[{dim}],
          +chunk_id TEXT
        );
        """
    )


def mock_embed_enabled() -> bool:
    return os.environ.get(MOCK_EMBED_ENV) == "1"


def insert_chunk_embedding(con: sqlite3.Connection, chunk_id: str, vec: list[float]) -> None:
    dim = table_dimension(con)
    if dim is None:
        raise ValueError("chunk_embeddings table is missing")
    n = len(vec)
    if n != dim:
        raise ValueError(
            f"embedding has {n} values but the index uses {dim}; rebuild the index"
        )
    blob = sqlite_vec.serialize_float32(list(vec))
    con.execute(
        "INSERT INTO chunk_embeddings(embedding, chunk_id) VALUES (?, ?)",
        (blob, chunk_id),
    )


def delete_embeddings_for_chunk_ids(con: sqlite3.Connection, chunk_ids: list[str]) -> None:
    for cid in chunk_ids:
        con.execute("DELETE FROM chunk_embeddings WHERE chunk_id = ?", (cid,))
