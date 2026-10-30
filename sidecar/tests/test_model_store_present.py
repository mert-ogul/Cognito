from pathlib import Path

from common.model_store import _model_present, models_status


def test_model_present_accepts_gliner_config(tmp_path: Path) -> None:
    gliner = tmp_path / "urchade--gliner_multi-v2.1"
    gliner.mkdir()
    (gliner / "gliner_config.json").write_text("{}", encoding="utf-8")
    assert _model_present(gliner) is True


def test_model_present_accepts_config_json(tmp_path: Path) -> None:
    rerank = tmp_path / "cross-encoder--mmarco-mMiniLMv2-L12-H384-v1"
    rerank.mkdir()
    (rerank / "config.json").write_text("{}", encoding="utf-8")
    assert _model_present(rerank) is True


def test_models_status_ready_with_minilm_and_hhem(tmp_path: Path) -> None:
    root = tmp_path / "models"
    for folder in (
        "cross-encoder--mmarco-mMiniLMv2-L12-H384-v1",
        "vectara--hallucination_evaluation_model",
    ):
        path = root / folder
        path.mkdir(parents=True)
        (path / "config.json").write_text("{}", encoding="utf-8")

    status = models_status(str(tmp_path))
    assert status["ready"] is True
    assert all(m["present"] for m in status["models"])
    assert len(status["models"]) == 2


def test_models_status_thorough_requires_qwen(tmp_path: Path) -> None:
    root = tmp_path / "models"
    for folder in (
        "cross-encoder--mmarco-mMiniLMv2-L12-H384-v1",
        "vectara--hallucination_evaluation_model",
    ):
        path = root / folder
        path.mkdir(parents=True)
        (path / "config.json").write_text("{}", encoding="utf-8")

    assert models_status(str(tmp_path), thorough_reranking=True)["ready"] is False
    qwen = root / "Qwen--Qwen3-Reranker-0.6B"
    qwen.mkdir()
    (qwen / "config.json").write_text("{}", encoding="utf-8")
    assert models_status(str(tmp_path), thorough_reranking=True)["ready"] is True
