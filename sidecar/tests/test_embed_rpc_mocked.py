"""embed_chunks with COGNITO_MOCK_EMBED (no Ollama)."""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import sys
from pathlib import Path

import pytest

_ROOT = Path(__file__).resolve().parents[1]
_MAIN = _ROOT / "main.py"


@pytest.fixture
def mock_embed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("COGNITO_MOCK_EMBED", "1")


def test_embed_chunks_via_subprocess(tmp_path: Path, mock_embed: None) -> None:
    db = tmp_path / "v.sqlite"
    db.write_bytes(b"")
    con = sqlite3.connect(db)
    con.executescript(
        """
        PRAGMA foreign_keys = ON;
        CREATE TABLE documents (
          id TEXT PRIMARY KEY,
          path TEXT NOT NULL UNIQUE,
          title TEXT NOT NULL,
          file_type TEXT NOT NULL,
          content_hash TEXT NOT NULL,
          page_count INTEGER,
          status TEXT NOT NULL,
          error_message TEXT,
          imported_at TEXT NOT NULL,
          indexed_at TEXT,
          modified_at TEXT NOT NULL
        );
        CREATE TABLE chunks (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
          ordinal INTEGER NOT NULL,
          page_start INTEGER,
          page_end INTEGER,
          text TEXT NOT NULL,
          token_count INTEGER NOT NULL,
          content_hash TEXT NOT NULL,
          created_at TEXT NOT NULL
        );
        """
    )
    con.execute(
        "INSERT INTO documents VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        (
            "d1",
            "/x.md",
            "x",
            "markdown",
            "h",
            None,
            "indexing",
            None,
            "2020-01-01T00:00:00Z",
            None,
            "2020-01-01T00:00:00Z",
        ),
    )
    con.execute(
        "INSERT INTO chunks VALUES (?,?,?,?,?,?,?,?,?)",
        (
            "c1",
            "d1",
            0,
            None,
            None,
            "hi",
            1,
            "hh",
            "2020-01-01T00:00:00Z",
        ),
    )
    con.commit()
    con.close()

    req = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "embed_chunks",
            "params": {
                "sqlitePath": str(db),
                "embedModel": "nomic-embed-text",
                "documentId": "d1",
                "items": [{"chunkId": "c1", "text": "hello world", "ordinal": 0}],
            },
        }
    )
    env = os.environ.copy()
    env["COGNITO_MOCK_EMBED"] = "1"
    py = sys.executable
    p = subprocess.run(
        [py, str(_MAIN)],
        input=req + "\n",
        capture_output=True,
        text=True,
        cwd=str(_ROOT),
        env=env,
        check=False,
    )
    assert p.returncode == 0, p.stderr
    line = json.loads(p.stdout.strip().splitlines()[0])
    assert "result" in line
    assert line["result"].get("ok") is True
