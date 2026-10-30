"""Rerank budget, sigmoid scores, Qwen CPU refusal."""

from __future__ import annotations

import time
from typing import Any

import pytest

from common import model_store as ms


def test_sigmoid_scores_between_0_and_1() -> None:
    for x in (-20.0, -1.0, 0.0, 1.0, 20.0):
        s = ms.sigmoid(x)
        assert 0.0 <= s <= 1.0


def test_rerank_budget_keeps_fused_order_for_unscored(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(ms._MOCK_MODELS_ENV, raising=False)

    class SlowMiniLm:
        def predict(self, pairs: list[Any], batch_size: int = 8, show_progress_bar: bool = False):
            time.sleep(1.2)
            return [float(i) for i in range(len(pairs))]

    monkeypatch.setattr(ms, "_load_minilm", lambda _app: SlowMiniLm())
    monkeypatch.setattr(ms, "_RERANK_BUDGET_S", 2.0)
    monkeypatch.setattr(ms, "_RERANK_BATCH", 8)

    docs = [f"passage {i}" for i in range(20)]
    out = ms.rerank_with_budget("/tmp", "q", docs, thorough_reranking=False)
    assert out["scored"] < 20
    assert out["budgetHit"] is True
    scores = out["scores"]
    assert len(scores) == 20
    # Scored batch values are sigmoid of 0..; unscored keep fused order via negative floors.
    scored_n = out["scored"]
    for i in range(scored_n, 20):
        assert scores[i] < 0
    # Unscored retain fused order (later index more negative).
    if scored_n < 19:
        assert scores[scored_n] > scores[scored_n + 1]


def test_qwen_reranker_refuses_cpu(monkeypatch: pytest.MonkeyPatch, tmp_path) -> None:
    monkeypatch.delenv(ms._MOCK_MODELS_ENV, raising=False)
    monkeypatch.setattr(ms, "_mps_available", lambda: False)
    dest = tmp_path / "models" / "Qwen--Qwen3-Reranker-0.6B"
    dest.mkdir(parents=True)
    (dest / "config.json").write_text("{}", encoding="utf-8")
    with pytest.raises(RuntimeError, match="refusing CPU"):
        ms._load_qwen_reranker(str(tmp_path))
