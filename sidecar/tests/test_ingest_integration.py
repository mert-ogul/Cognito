"""Integration: parse → chunk → embed (mock) into temp SQLite."""

from __future__ import annotations

import sqlite3
from pathlib import Path

import pytest

from common.sqlite_vec_util import connect_with_vec, ensure_chunk_embedding_table
from ingestion.chunking import chunk_semantic, estimate_tokens, hash_chunk_text
from ingestion.parsing import parse_file
from ingestion.rpc_ingest import rpc_embed_chunks

_REPO_ROOT = Path(__file__).resolve().parents[2]
_MIGRATION = _REPO_ROOT / "src-tauri" / "migrations" / "001_initial.sql"


@pytest.fixture
def mock_embed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("COGNITO_MOCK_EMBED", "1")


def test_five_markdown_files_indexed(tmp_path: Path, mock_embed: None) -> None:
    vault = tmp_path / "vault"
    vault.mkdir()
    for i in range(5):
        (vault / f"n{i}.md").write_text(f"# Doc {i}\n\n" + ("paragraph " * 80), encoding="utf-8")

    db = tmp_path / "db.sqlite"
    sql = _MIGRATION.read_text(encoding="utf-8")
    con = sqlite3.connect(db)
    con.executescript(sql)
    con.execute("DROP TABLE IF EXISTS chunk_embeddings")
    con.commit()
    con.close()

    paths = sorted(vault.glob("*.md"))
    assert len(paths) == 5
    for p in paths:
        parsed = parse_file(p)
        doc_id = f"id-{p.stem}"
        con = sqlite3.connect(db)
        now = "2020-01-01T00:00:00Z"
        con.execute(
            """INSERT INTO documents (id, path, title, file_type, content_hash, page_count,
               status, error_message, imported_at, indexed_at, modified_at)
               VALUES (?,?,?,?,?,?,?,?,?,?,?)""",
            (
                doc_id,
                str(p.resolve()),
                parsed["title"],
                parsed["fileType"],
                parsed["contentHash"],
                parsed["pageCount"],
                "indexing",
                None,
                now,
                None,
                now,
            ),
        )
        for ord_i, ch in enumerate(chunk_semantic(parsed["text"], parsed["fileType"])):
            cid = f"{doc_id}-c{ord_i}"
            con.execute(
                """INSERT INTO chunks (id, document_id, ordinal, page_start, page_end, text,
                   token_count, content_hash, created_at)
                   VALUES (?,?,?,?,?,?,?,?,?)""",
                (
                    cid,
                    doc_id,
                    ord_i,
                    None,
                    None,
                    ch.text,
                    estimate_tokens(ch.text),
                    hash_chunk_text(ch.text),
                    now,
                ),
            )
        con.commit()
        con.close()
        con = sqlite3.connect(db)
        rows = [
            {"chunkId": r[0], "text": r[1], "ordinal": r[2]}
            for r in con.execute(
                "SELECT id, text, ordinal FROM chunks WHERE document_id = ? ORDER BY ordinal",
                (doc_id,),
            )
        ]
        con.close()
        out = rpc_embed_chunks(
            {
                "sqlitePath": str(db),
                "embedModel": "nomic-embed-text",
                "documentId": doc_id,
                "items": rows,
            }
        )
        assert out.get("ok") is True

    con = connect_with_vec(str(db))
    ensure_chunk_embedding_table(con)
    n_vec = con.execute("SELECT COUNT(*) FROM chunk_embeddings").fetchone()[0]
    con.close()
    n_chunks = sqlite3.connect(db).execute("SELECT COUNT(*) FROM chunks").fetchone()[0]
    assert n_chunks > 0
    assert n_vec == n_chunks
    assert n_vec >= 5
