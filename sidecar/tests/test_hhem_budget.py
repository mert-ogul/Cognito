"""HHEM budget yields unchecked statuses."""

from __future__ import annotations

import time

import pytest

from common import model_store as ms
from query.check_answer import check_answer


def test_hhem_budget_marks_unchecked(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(ms._MOCK_MODELS_ENV, raising=False)

    class SlowHhem:
        def predict(self, pairs):
            time.sleep(1.5)
            return [0.9] * len(pairs)

    monkeypatch.setattr(ms, "_load_hhem", lambda _app: SlowHhem())
    monkeypatch.setattr(ms, "_HHEM_BUDGET_S", 2.0)

    pairs = [("a", "b"), ("c", "d"), ("e", "f")]
    scores = ms.hhem_scores_bounded("/tmp", pairs)
    assert scores[0] == pytest.approx(0.9)
    assert any(s is None for s in scores)


def test_check_answer_unchecked_status(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ms._MOCK_MODELS_ENV, "1")

    def fake_bounded(_app, pairs):
        return [None] * len(pairs)

    monkeypatch.setattr("query.check_answer.hhem_scores_bounded", fake_bounded)

    passages = [{"index": 1, "text": "The model uses Cora."}]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [{"source": 1, "text": "uses Cora"}],
                "sentence": "It uses Cora.",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["sentences"][0]["status"] == "unchecked"
    assert out["sentences"][0]["reason"] == "budget"
    assert out["notFound"] is False


def test_check_answer_hhem_exception_is_error(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(ms._MOCK_MODELS_ENV, "1")

    def fake_bounded(_app, pairs):
        return [float("nan")] * len(pairs)

    monkeypatch.setattr("query.check_answer.hhem_scores_bounded", fake_bounded)

    passages = [{"index": 1, "text": "The model uses Cora."}]
    draft = {
        "answerable": True,
        "answer": [
            {
                "quotes": [{"source": 1, "text": "uses Cora"}],
                "sentence": "It uses Cora.",
            }
        ],
    }
    out = check_answer("/tmp", passages, draft)
    assert out["sentences"][0]["status"] == "unchecked"
    assert out["sentences"][0]["reason"] == "error"
    assert out["notFound"] is False
