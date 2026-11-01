"""Rerank budget and sigmoid scores."""

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
    out = ms.rerank_with_budget("/tmp", "q", docs)
    assert out["scored"] < 20
    assert out["budgetHit"] is True
    scores = out["scores"]
    assert len(scores) == 20
    scored_n = out["scored"]
    for i in range(scored_n, 20):
        assert scores[i] < 0
    if scored_n < 19:
        assert scores[scored_n] > scores[scored_n + 1]
