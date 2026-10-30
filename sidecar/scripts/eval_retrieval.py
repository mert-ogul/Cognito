#!/usr/bin/env python3
"""
Retrieval eval against a Cognito vault.sqlite.

Runs prepare_chat_retrieval per line in eval/questions.jsonl and reports
hit@1 / hit@5 (title or file match) and notFound accuracy when expect_file is null.

Usage (from repo root):
  npm run eval
  cd sidecar && uv run python scripts/eval_retrieval.py [--sqlite PATH] [--questions PATH]
      [--embed-model M] [--app-data PATH] [--mock-models]

Rerank needs local models under app_data/models (or pass --mock-models /
COGNITO_MOCK_MODELS=1). Embedding uses Ollama unless COGNITO_MOCK_EMBED=1.
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
    try:
        con = sqlite3.connect(str(sqlite_path))
        try:
            row = con.execute(
                "SELECT value FROM settings WHERE key = ?", (key,)
            ).fetchone()
            return str(row[0]) if row and row[0] is not None else None
        finally:
            con.close()
    except sqlite3.Error:
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


def _citation_matches(
    cite: dict[str, Any],
    *,
    expect_file: str | None,
    expect_title: str | None,
    expect_page: int | None,
) -> bool:
    title = str(cite.get("title") or "")
    path = str(cite.get("path") or "")
    basename = Path(path).name

    title_ok = bool(expect_title) and expect_title.lower() in title.lower()
    file_ok = bool(expect_file) and basename.lower() == expect_file.lower()
    if not (title_ok or file_ok):
        return False
    if expect_page is not None:
        page = cite.get("pageStart")
        try:
            return page is not None and int(page) == int(expect_page)
        except (TypeError, ValueError):
            return False
    return True


def _hit_at_k(
    citations: list[dict[str, Any]],
    k: int,
    *,
    expect_file: str | None,
    expect_title: str | None,
    expect_page: int | None,
) -> bool:
    for cite in citations[:k]:
        if _citation_matches(
            cite,
            expect_file=expect_file,
            expect_title=expect_title,
            expect_page=expect_page,
        ):
            return True
    return False


def _ensure_rerank_ready(app_data: str, mock: bool) -> None:
    if mock or os.environ.get("COGNITO_MOCK_MODELS") == "1":
        return
    from common.model_store import MINILM_RERANKER_REPO, models_status

    status = models_status(app_data)
    rerank = next((m for m in status["models"] if m["name"] == MINILM_RERANKER_REPO), None)
    if rerank and rerank.get("present"):
        return
    raise SystemExit(
        "Reranker model missing under "
        f"{Path(app_data) / 'models'}. "
        "Install models in Cognito, or re-run with --mock-models "
        "(sets COGNITO_MOCK_MODELS=1; scores are synthetic)."
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
        "--mock-models",
        action="store_true",
        help="Set COGNITO_MOCK_MODELS=1 before retrieval (no local reranker download)",
    )
    parser.add_argument(
        "--trace",
        action="store_true",
        help="Print retrieval trace for each question that misses (notFound or hit@1 fail)",
    )
    args = parser.parse_args()

    if args.mock_models:
        os.environ["COGNITO_MOCK_MODELS"] = "1"

    sqlite_path = (args.sqlite or _default_sqlite()).expanduser().resolve()
    if not sqlite_path.is_file():
        raise SystemExit(
            f"SQLite not found: {sqlite_path}\n"
            "Pass --sqlite PATH or set COGNITO_SQLITE."
        )

    app_data = (
        args.app_data.expanduser().resolve()
        if args.app_data
        else sqlite_path.parent
    )
    questions_path = args.questions.expanduser().resolve()
    if not questions_path.is_file():
        raise SystemExit(f"Questions file not found: {questions_path}")

    embed_model = (
        args.embed_model
        or os.environ.get("COGNITO_EMBED_MODEL")
        or _read_setting(sqlite_path, "embed_model")
        or _DEFAULT_EMBED
    )

    _ensure_rerank_ready(str(app_data), mock=args.mock_models)

    # Import after env flags so mock_models_enabled() sees COGNITO_MOCK_MODELS.
    from query.chat_pipeline import prepare_chat_retrieval

    rows = _load_questions(questions_path)
    if not rows:
        raise SystemExit(f"No questions in {questions_path}")

    hit1_scores: list[float] = []
    hit5_scores: list[float] = []
    not_found_scores: list[float] = []

    print(f"sqlite: {sqlite_path}")
    print(f"app_data: {app_data}")
    print(f"embed_model: {embed_model}")
    print(f"mock_models: {os.environ.get('COGNITO_MOCK_MODELS') == '1'}")
    print(f"questions: {questions_path} ({len(rows)})")
    print()

    for i, row in enumerate(rows, 1):
        question = str(row["question"])
        expect_file = row.get("expect_file")
        expect_title = row.get("expect_title")
        expect_page = row.get("expect_page")
        if expect_file is not None:
            expect_file = str(expect_file)
        if expect_title is not None:
            expect_title = str(expect_title)
        if expect_page is not None:
            expect_page = int(expect_page)

        expects_not_found = expect_file is None and not expect_title

        out = prepare_chat_retrieval(
            str(sqlite_path),
            question,
            embed_model,
            app_data=str(app_data),
        )
        citations = list(out.get("citations") or [])
        not_found = bool(out.get("notFound"))
        best = out.get("bestScore")

        if expects_not_found:
            ok = 1.0 if not_found else 0.0
            not_found_scores.append(ok)
            print(
                f"[{i}] notFound={'PASS' if ok else 'FAIL'} "
                f"(got notFound={not_found}, bestScore={best}) | {question!r}"
            )
            if args.trace and not ok:
                print(json.dumps(out.get("trace") or {}, indent=2, default=str))
            continue

        h1 = _hit_at_k(
            citations,
            1,
            expect_file=expect_file,
            expect_title=expect_title,
            expect_page=expect_page,
        )
        h5 = _hit_at_k(
            citations,
            5,
            expect_file=expect_file,
            expect_title=expect_title,
            expect_page=expect_page,
        )
        hit1_scores.append(1.0 if h1 else 0.0)
        hit5_scores.append(1.0 if h5 else 0.0)
        top = citations[0] if citations else None
        top_desc = ""
        if top:
            top_desc = (
                f" top1={top.get('title')!r} "
                f"file={Path(str(top.get('path') or '')).name!r} "
                f"page={top.get('pageStart')}"
            )
        print(
            f"[{i}] hit@1={'1' if h1 else '0'} hit@5={'1' if h5 else '0'} "
            f"notFound={not_found} bestScore={best}{top_desc} | {question!r}"
        )
        if args.trace and (not h1 or not_found):
            print(json.dumps(out.get("trace") or {}, indent=2, default=str))

    def _avg(xs: list[float]) -> float | None:
        return sum(xs) / len(xs) if xs else None

    print()
    print("--- averages ---")
    a1 = _avg(hit1_scores)
    a5 = _avg(hit5_scores)
    anf = _avg(not_found_scores)
    if a1 is not None:
        print(f"hit@1: {a1:.3f} (n={len(hit1_scores)})")
    else:
        print("hit@1: n/a (no expect_file/expect_title questions)")
    if a5 is not None:
        print(f"hit@5: {a5:.3f} (n={len(hit5_scores)})")
    else:
        print("hit@5: n/a (no expect_file/expect_title questions)")
    if anf is not None:
        print(f"notFound accuracy: {anf:.3f} (n={len(not_found_scores)})")
    else:
        print("notFound accuracy: n/a (no expect_file=null questions)")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
