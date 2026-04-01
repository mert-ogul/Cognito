#!/usr/bin/env python3
"""
Measure import/index pipeline phases on a fixture corpus (local dev / RC).

Phases (seconds):
  parse, chunk, sqlite insert, embed, graph extraction (optional), query (optional)

Requires: sidecar deps (`uv sync`). Default uses COGNITO_MOCK_EMBED=1 so Ollama is not needed
for parse/chunk/embed timings. Graph extraction still calls the LLM unless --skip-graph.

Memory: `getrusage` max RSS at start/end (`memoryMaxRssBytes`); **macOS reports bytes**, Linux reports KiB converted to bytes in the script.
"""

from __future__ import annotations

import argparse
import json
import os
import resource
import shutil
import sys
import tempfile
import time
from pathlib import Path

# ru_maxrss: bytes on macOS, kilobytes on Linux (see getrusage(2)).

# Repo root: sidecar/scripts -> parents[1] == sidecar
_ROOT = Path(__file__).resolve().parents[1]
if str(_ROOT) not in sys.path:
    sys.path.insert(0, str(_ROOT))

from ingestion.chunking import estimate_tokens, hash_chunk_text
from ingestion.rpc_graph import rpc_extract_graph_document
from ingestion.rpc_ingest import rpc_chunk_documents, rpc_embed_chunks, rpc_parse_documents
from query.chat_pipeline import prepare_chat_retrieval

_REPO_ROOT = _ROOT.parent
_MIGRATION = _REPO_ROOT / "src-tauri" / "migrations" / "001_initial.sql"


def _rss_bytes() -> int:
    try:
        raw = int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    except (AttributeError, ValueError):
        return 0
    if sys.platform == "darwin":
        return raw
    return raw * 1024


def _prepare_corpus(vault: Path, count: int, fixture: Path, corpus_dir: Path | None) -> list[Path]:
    if corpus_dir and corpus_dir.is_dir():
        files = sorted(corpus_dir.glob("*.md")) + sorted(corpus_dir.glob("*.pdf"))
        return files[:count]
    text = fixture.read_text(encoding="utf-8")
    out: list[Path] = []
    for i in range(count):
        p = vault / f"bench_{i}.md"
        p.write_text(f"# Bench {i}\n\n{text}", encoding="utf-8")
        out.append(p)
    return out


def _init_sqlite(db: Path) -> None:
    sql = _MIGRATION.read_text(encoding="utf-8")
    import sqlite3

    con = sqlite3.connect(db)
    con.executescript(sql)
    con.execute("DROP TABLE IF EXISTS chunk_embeddings")
    con.commit()
    con.close()


def main() -> int:
    parser = argparse.ArgumentParser(description="Benchmark Cognito import/index pipeline")
    parser.add_argument("--count", type=int, default=10, help="Number of documents (max 100)")
    parser.add_argument(
        "--fixture",
        type=Path,
        default=_ROOT / "tests" / "fixtures" / "sample.md",
        help="Markdown template when not using --corpus-dir",
    )
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=None,
        help="Optional folder of .md/.pdf files (first N used)",
    )
    parser.add_argument(
        "--mock-embed",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Use COGNITO_MOCK_EMBED=1 (default: true)",
    )
    parser.add_argument(
        "--graph",
        action="store_true",
        help="Time graph extraction (requires Ollama + chat model; slow)",
    )
    parser.add_argument("--skip-query", action="store_true", help="Skip prepare_chat_retrieval timing")
    parser.add_argument(
        "--chat-model",
        default=os.environ.get("COGNITO_BENCH_CHAT_MODEL", "phi4-mini"),
        help="Chat model for graph extraction",
    )
    parser.add_argument(
        "--embed-model",
        default=os.environ.get("COGNITO_BENCH_EMBED_MODEL", "nomic-embed-text"),
        help="Embedding model name (for embed + query phases)",
    )
    args = parser.parse_args()
    n = max(1, min(100, args.count))

    if args.mock_embed:
        os.environ["COGNITO_MOCK_EMBED"] = "1"

    rss0 = _rss_bytes()
    t0 = time.perf_counter()

    tmp = tempfile.mkdtemp(prefix="cognito-bench-")
    try:
        vault = Path(tmp) / "vault"
        vault.mkdir()
        db = Path(tmp) / "vault.sqlite"
        duck = Path(tmp) / "graph.duckdb"

        paths = _prepare_corpus(vault, n, args.fixture, args.corpus_dir)
        if not paths:
            print(json.dumps({"error": "no corpus files"}, indent=2))
            return 1

        # --- parse (batch RPC) ---
        t_parse0 = time.perf_counter()
        vault_root = str(paths[0].parent.resolve())
        abs_paths = [str(p.resolve()) for p in paths]
        parsed = rpc_parse_documents({"vaultRoot": vault_root, "paths": abs_paths})
        parse_s = time.perf_counter() - t_parse0
        items = parsed.get("items") or []
        errs = parsed.get("errors") or []

        # --- chunk ---
        t_chunk0 = time.perf_counter()
        chunked = rpc_chunk_documents({"items": items})
        chunk_s = time.perf_counter() - t_chunk0
        chunk_items = chunked.get("items") or []
        chunk_total = sum(len(x.get("chunks") or []) for x in chunk_items)

        _init_sqlite(db)
        import sqlite3

        now = "2020-01-01T00:00:00Z"
        doc_ids: list[str] = []
        by_path = {str(Path(it["path"]).resolve()): it for it in items}

        # --- insert rows + embed per document ---
        t_db0 = time.perf_counter()
        t_embed_total = 0.0
        for idx, pack in enumerate(chunk_items):
            path = pack["path"]
            p = Path(path)
            key = str(p.resolve())
            src = by_path.get(key) or by_path.get(path)
            if not src:
                continue
            doc_id = f"bench-doc-{idx}"
            doc_ids.append(doc_id)
            con = sqlite3.connect(db)
            con.execute(
                """INSERT INTO documents (id, path, title, file_type, content_hash, page_count,
                   status, error_message, imported_at, indexed_at, modified_at)
                   VALUES (?,?,?,?,?,?,?,?,?,?,?)""",
                (
                    doc_id,
                    key,
                    src["title"],
                    src["fileType"],
                    src["contentHash"],
                    src["pageCount"],
                    "indexing",
                    None,
                    now,
                    None,
                    now,
                ),
            )
            for ord_i, ch in enumerate(pack.get("chunks") or []):
                text = str(ch.get("text", ""))
                cid = f"{doc_id}-c{ord_i}"
                ps = ch.get("pageStart")
                pe = ch.get("pageEnd")
                con.execute(
                    """INSERT INTO chunks (id, document_id, ordinal, page_start, page_end, text,
                       token_count, content_hash, created_at)
                       VALUES (?,?,?,?,?,?,?,?,?)""",
                    (
                        cid,
                        doc_id,
                        ord_i,
                        ps,
                        pe,
                        text,
                        estimate_tokens(text),
                        hash_chunk_text(text),
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
            t_e0 = time.perf_counter()
            rpc_embed_chunks(
                {
                    "sqlitePath": str(db),
                    "embedModel": args.embed_model,
                    "documentId": doc_id,
                    "items": rows,
                }
            )
            t_embed_total += time.perf_counter() - t_e0

        db_s = time.perf_counter() - t_db0 - t_embed_total
        embed_s = t_embed_total

        graph_s = None
        graph_chunks = 0
        if args.graph:
            t_g0 = time.perf_counter()
            for doc_id in doc_ids:
                out = rpc_extract_graph_document(
                    {
                        "sqlitePath": str(db),
                        "duckPath": str(duck),
                        "documentId": doc_id,
                        "chatModel": args.chat_model,
                    }
                )
                graph_chunks += int(out.get("chunksProcessed") or 0)
            graph_s = time.perf_counter() - t_g0

        query_s = None
        if not args.skip_query:
            t_q0 = time.perf_counter()
            for q in ("What is this corpus about?", "List main topics.", "Summarize sources."):
                prepare_chat_retrieval(
                    str(db),
                    str(duck) if duck.exists() else None,
                    q,
                    args.embed_model,
                )
            query_s = time.perf_counter() - t_q0

        total_s = time.perf_counter() - t0
        rss1 = _rss_bytes()

        report = {
            "documents": len(paths),
            "parseErrors": len(errs),
            "chunkCount": chunk_total,
            "timingsSeconds": {
                "parse": round(parse_s, 4),
                "chunk": round(chunk_s, 4),
                "sqliteInsert": round(db_s, 4),
                "embed": round(embed_s, 4),
                "importTotalExGraphQuery": round(parse_s + chunk_s + db_s + embed_s, 4),
            },
            "memoryMaxRssBytes": {
                "start": rss0,
                "end": rss1,
                "delta": (rss1 - rss0) if rss0 and rss1 else None,
                "deltaMib": round((rss1 - rss0) / (1024 * 1024), 2) if rss0 and rss1 else None,
            },
            "totalWallSeconds": round(total_s, 4),
            "mockEmbed": args.mock_embed,
        }
        if graph_s is not None:
            report["timingsSeconds"]["graphExtraction"] = round(graph_s, 4)
            report["graphChunksProcessed"] = graph_chunks
        if query_s is not None:
            report["timingsSeconds"]["queryLatencyThreeCalls"] = round(query_s, 4)
            report["timingsSeconds"]["queryLatencyAvgPerCall"] = round(query_s / 3.0, 4)

        print(json.dumps(report, indent=2))
        return 0 if not errs else 0
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
