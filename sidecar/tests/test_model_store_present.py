from pathlib import Path

from common.model_store import _model_present, models_status


def test_model_present_requires_complete(tmp_path: Path) -> None:
    gliner = tmp_path / "urchade--gliner_multi-v2.1"
    gliner.mkdir()
    (gliner / "gliner_config.json").write_text("{}", encoding="utf-8")
    assert _model_present(gliner) is False
    (gliner / ".complete").write_text("", encoding="utf-8")
    assert _model_present(gliner) is True


def test_model_folder_without_complete_is_missing(tmp_path: Path) -> None:
    root = tmp_path / "models"
    for folder in (
        "cross-encoder--mmarco-mMiniLMv2-L12-H384-v1",
        "vectara--hallucination_evaluation_model",
        "urchade--gliner_multi-v2.1",
    ):
        path = root / folder
        path.mkdir(parents=True)
        (path / "config.json").write_text("{}", encoding="utf-8")

    status = models_status(str(tmp_path))
    assert status["ready"] is False
    assert all(m["present"] is False for m in status["models"])


def test_models_status_ready_with_complete_markers(tmp_path: Path) -> None:
    root = tmp_path / "models"
    for folder in (
        "cross-encoder--mmarco-mMiniLMv2-L12-H384-v1",
        "vectara--hallucination_evaluation_model",
        "urchade--gliner_multi-v2.1",
    ):
        path = root / folder
        path.mkdir(parents=True)
        (path / "config.json").write_text("{}", encoding="utf-8")
        (path / ".complete").write_text("", encoding="utf-8")

    status = models_status(str(tmp_path))
    assert status["ready"] is True
    assert all(m["present"] for m in status["models"])
    assert len(status["models"]) == 3
