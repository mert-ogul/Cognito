#!/usr/bin/env python3
"""
Retrieval eval against a Cognito vault.sqlite.

Runs prepare_chat_retrieval (skip_rerank) then rerank_chat_candidates the same
way chat_query.rs does, and reports bestScore, notFound, and top title/page.

Usage (from repo root):
  npm run eval
  cd sidecar && uv run python scripts/eval_retrieval.py [--sqlite PATH] [--questions PATH]
      [--embed-model M] [--app-data PATH]

Rerank needs local models under app_data/models. Embedding uses Ollama unless
COGNITO_MOCK_EMBED=1.
"""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys
from pathlib import Path
from typing import Any

_SIDECAR = Path(__file__).resolve().parents[1]
_REPO = _SIDECAR.parent
if str(_SIDECAR) not in sys.path:
    sys.path.insert(0, str(_SIDECAR))

_BUNDLE_ID = "com.mertogul.cognito"
_DEFAULT_EMBED = "qwen3-embedding:0.6b"


def _default_app_data() -> Path:
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / _BUNDLE_ID
    if sys.platform == "win32":
        base = os.environ.get("APPDATA") or str(Path.home() / "AppData" / "Roaming")
        return Path(base) / _BUNDLE_ID
    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg) / _BUNDLE_ID
    return Path.home() / ".local" / "share" / _BUNDLE_ID


def _default_sqlite() -> Path:
    env = os.environ.get("COGNITO_SQLITE")
    if env:
        return Path(env).expanduser()
    return _default_app_data() / "vault.sqlite"


def _read_setting(sqlite_path: Path, key: str) -> str | None:
    from common.sqlite_vec_util import require_sqlite_path

    try:
        path = require_sqlite_path(str(sqlite_path) if sqlite_path is not None else None)
        con = sqlite3.connect(path)
        try:
            row = con.execute("SELECT value FROM settings WHERE key = ?", (key,)).fetchone()
            return str(row[0]) if row and row[0] is not None else None
        finally:
            con.close()
    except (sqlite3.Error, ValueError):
        return None


def _load_questions(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as f:
        for line_no, line in enumerate(f, 1):
            text = line.strip()
            if not text or text.startswith("#"):
                continue
            try:
                obj = json.loads(text)
            except json.JSONDecodeError as e:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {e}") from e
            if not isinstance(obj, dict) or "question" not in obj:
                raise SystemExit(f"{path}:{line_no}: need object with 'question'")
            rows.append(obj)
    return rows


def _ensure_rerank_ready(app_data: str) -> None:
    from common.model_store import MINILM_RERANKER_REPO, models_status

    status = models_status(app_data)
    rerank = next((m for m in status["models"] if m["name"] == MINILM_RERANKER_REPO), None)
    if rerank and rerank.get("present"):
        return
    raise SystemExit(
        "Reranker model missing under "
        f"{Path(app_data) / 'models'}. "
        "Install models in Cognito before running eval."
    )


def _run_question(
    *,
    sqlite_path: str,
    question: str,
    embed_model: str,
    app_data: str,
) -> dict[str, Any]:
    from query.chat_pipeline import prepare_chat_retrieval, rerank_chat_candidates

    prep = prepare_chat_retrieval(
        sqlite_path,
        question,
        embed_model,
        app_data=app_data,
        skip_rerank=True,
    )
    candidates = list(prep.get("candidates") or [])
    if not candidates:
        candidates = list(prep.get("citations") or [])
    if not candidates:
        return {
            "citations": [],
            "notFound": True,
            "bestScore": float(prep.get("bestScore") or 0.0),
            "trace": prep.get("trace"),
        }
    return rerank_chat_candidates(
        app_data,
        str(prep.get("searchText") or question),
        candidates,
        sqlite_path=sqlite_path,
        embed_model=embed_model,
        profile_matches=list(prep.get("profileMatches") or []),
        explicit_scope=False,
        labels=list(prep.get("labels") or []),
        normalised_question=str(prep.get("normalisedQuestion") or question),
        scope_ids=list(prep.get("scopeIds") or []),
        scope_reason=str(prep.get("scopeReason") or ""),
        candidate_counts=dict(prep.get("candidateCounts") or {}),
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Evaluate Cognito retrieval hit rates")
    parser.add_argument(
        "--sqlite",
        type=Path,
        default=None,
        help="Path to vault.sqlite (default: COGNITO_SQLITE or app data dir)",
    )
    parser.add_argument(
        "--questions",
        type=Path,
        default=_REPO / "eval" / "questions.jsonl",
        help="JSONL questions file",
    )
    parser.add_argument(
        "--embed-model",
        default=None,
        help="Embedding model (default: settings.embed_model or env COGNITO_EMBED_MODEL)",
    )
    parser.add_argument(
        "--app-data",
        type=Path,
        default=None,
        help="App data dir for models (default: parent of sqlite)",
    )
    parser.add_argument(
        "--trace",
        action="store_true",
        help="Print retrieval trace for each question that misses",
    )
    args = parser.parse_args()

    raw_sqlite = args.sqlite or _default_sqlite()
    if raw_sqlite is None:
        raise SystemExit("SQLite path is missing.\nPass --sqlite PATH or set COGNITO_SQLITE.")
    sqlite_path = raw_sqlite.expanduser().resolve()
    if not sqlite_path.is_file():
        raise SystemExit(
            f"SQLite not found: {sqlite_path}\nPass --sqlite PATH or set COGNITO_SQLITE."
        )

    app_data = args.app_data.expanduser().resolve() if args.app_data else sqlite_path.parent
    questions_path = args.questions.expanduser().resolve()
    if not questions_path.is_file():
        raise SystemExit(f"Questions file not found: {questions_path}")

    embed_model = (
        args.embed_model
        or os.environ.get("COGNITO_EMBED_MODEL")
        or _read_setting(sqlite_path, "embed_model")
        or _DEFAULT_EMBED
    )

    _ensure_rerank_ready(str(app_data))

    rows = _load_questions(questions_path)
    if not rows:
        raise SystemExit(f"No questions in {questions_path}")

    answerable_scores: list[float] = []
    unanswerable_scores: list[float] = []

    print(f"sqlite: {sqlite_path}")
    print(f"app_data: {app_data}")
    print(f"embed_model: {embed_model}")
    print(f"questions: {questions_path} ({len(rows)})")
    print()

    for i, row in enumerate(rows, 1):
        question = str(row["question"])
        expect_file = row.get("expect_file")
        expects_not_found = expect_file is None

        out = _run_question(
            sqlite_path=str(sqlite_path),
            question=question,
            embed_model=embed_model,
            app_data=str(app_data),
        )
        citations = list(out.get("citations") or [])
        not_found = bool(out.get("notFound"))
        best = float(out.get("bestScore") or 0.0)
        top = citations[0] if citations else None
        top_title = (top.get("title") if top else None) or ""
        top_page = top.get("pageStart") if top else None

        if expects_not_found:
            unanswerable_scores.append(best)
        else:
            answerable_scores.append(best)

        print(
            f"[{i}] bestScore={best:.4f} notFound={not_found} "
            f"title={top_title!r} page={top_page} | {question!r}"
        )
        if args.trace and (
            (expects_not_found and not not_found) or (not expects_not_found and not_found)
        ):
            print(json.dumps(out.get("trace") or {}, indent=2, default=str))

    print()
    if answerable_scores:
        print(f"lowest bestScore (answerable): {min(answerable_scores):.4f}")
    else:
        print("lowest bestScore (answerable): n/a")
    if unanswerable_scores:
        print(f"highest bestScore (unanswerable): {max(unanswerable_scores):.4f}")
    else:
        print("highest bestScore (unanswerable): n/a")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
