"""sqlite-vec virtual table helpers (nomic-embed-text = 768 dims)."""

from __future__ import annotations

import os
import sqlite3

import sqlite_vec

VEC_DIM = 768
MOCK_EMBED_ENV = "COGNITO_MOCK_EMBED"


def connect_with_vec(path: str) -> sqlite3.Connection:
    con = sqlite3.connect(path)
    con.execute("PRAGMA foreign_keys = ON")
    con.enable_load_extension(True)
    sqlite_vec.load(con)
    return con


def ensure_chunk_embedding_table(con: sqlite3.Connection) -> None:
    con.execute(
        f"""
        CREATE VIRTUAL TABLE IF NOT EXISTS chunk_embeddings USING vec0(
          embedding float[{VEC_DIM}],
          +chunk_id TEXT
        );
        """
    )


def mock_embed_enabled() -> bool:
    return os.environ.get(MOCK_EMBED_ENV) == "1"


def serialize_embedding(vec: list[float]) -> bytes:
    v = list(vec[:VEC_DIM])
    while len(v) < VEC_DIM:
        v.append(0.0)
    return sqlite_vec.serialize_float32(v[:VEC_DIM])


def insert_chunk_embedding(con: sqlite3.Connection, chunk_id: str, vec: list[float]) -> None:
    blob = serialize_embedding(vec)
    con.execute(
        "INSERT INTO chunk_embeddings(embedding, chunk_id) VALUES (?, ?)",
        (blob, chunk_id),
    )


def delete_embeddings_for_chunk_ids(con: sqlite3.Connection, chunk_ids: list[str]) -> None:
    for cid in chunk_ids:
        con.execute("DELETE FROM chunk_embeddings WHERE chunk_id = ?", (cid,))
