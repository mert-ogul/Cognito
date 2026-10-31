"""Download and lazy-load local checking models under app data."""

from __future__ import annotations

import logging
import math
import os
import re
import time
from pathlib import Path
from typing import Any, Callable

log = logging.getLogger("cognito.sidecar")

MINILM_RERANKER_REPO = "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1"
MINILM_RERANKER_REVISION = "1427fd652930e4ba29e8149678df786c240d8825"
# Back-compat alias used by older call sites / eval scripts.
RERANKER_REPO = MINILM_RERANKER_REPO
HHEM_REPO = "vectara/hallucination_evaluation_model"
HHEM_REVISION = "de9a5e6ba9ba49531a022dba17e35ffeb2a6fad8"
GLINER_REPO = "urchade/gliner_multi-v2.1"
GLINER_REVISION = "443d26d654e0324125a96bebd8e796c14ff2efe6"

FLAN_T5_REPO = "google/flan-t5-base"
MDEBERTA_REPO = "microsoft/mdeberta-v3-base"

_COMPLETE_NAME = ".complete"

_MOCK_MODELS_ENV = "COGNITO_MOCK_MODELS"

_RERANK_BATCH = 8
_RERANK_BUDGET_S = 3.0
_RERANK_MAX_LENGTH = 384
_RERANK_PASSAGE_TOKENS = 300
_HHEM_BUDGET_S = 6.0
_TORCH_THREADS = 4

MINILM_ALLOW = (
    "config.json",
    "model.safetensors",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "sentencepiece.bpe.model",
)
HHEM_ALLOW = (
    "config.json",
    "model.safetensors",
    "configuration_hhem_v2.py",
    "modeling_hhem_v2.py",
)
GLINER_ALLOW = (
    "gliner_config.json",
    "model.safetensors",
)
FLAN_T5_ALLOW = (
    "config.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "spiece.model",
)
MDEBERTA_ALLOW = (
    "config.json",
    "tokenizer_config.json",
    "spm.model",
)

# Local snapshots under models/<org--name>, each pinned.
_LOCAL_MODELS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    (MINILM_RERANKER_REPO, MINILM_RERANKER_REVISION, MINILM_ALLOW),
    (HHEM_REPO, HHEM_REVISION, HHEM_ALLOW),
    (GLINER_REPO, GLINER_REVISION, GLINER_ALLOW),
)
# Tokenizer / config deps in HF_HOME (float on main).
_CACHE_DEPS: tuple[tuple[str, tuple[str, ...]], ...] = (
    (FLAN_T5_REPO, FLAN_T5_ALLOW),
    (MDEBERTA_REPO, MDEBERTA_ALLOW),
)

# Lazy caches keyed by resolved app_data path.
_minilm_cache: dict[str, Any] = {}
_hhem_cache: dict[str, Any] = {}
_gliner_cache: dict[str, Any] = {}
_torch_threads_set = False

_MOCK_DATE_RE = re.compile(
    r"\b("
    r"\d{4}-\d{1,2}-\d{1,2}"
    r"|\d{1,2}[/-]\d{1,2}[/-]\d{4}"
    r"|(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec)[a-z]*"
    r"\s+\d{1,2}(?:st|nd|rd|th)?,?\s+\d{4}"
    r"|\d{4}"
    r")\b",
    re.IGNORECASE,
)
_MOCK_CAPITALISED_RE = re.compile(r"\b(?:[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*)\b")


def models_dir(app_data: str) -> Path:
    return Path(app_data) / "models"


def hf_home(app_data: str) -> Path:
    return models_dir(app_data) / "hf"


def mock_models_enabled() -> bool:
    return os.environ.get(_MOCK_MODELS_ENV) == "1"


def _repo_folder(repo_id: str) -> str:
    return repo_id.replace("/", "--")


def _snapshot_path(app_data: str, repo_id: str) -> Path:
    return models_dir(app_data) / _repo_folder(repo_id)


def _complete_path(app_data: str, repo_id: str) -> Path:
    return _snapshot_path(app_data, repo_id) / _COMPLETE_NAME


def _dir_size_bytes(path: Path) -> int:
    if not path.is_dir():
        return 0
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += (Path(root) / name).stat().st_size
            except OSError:
                continue
    return total


def _model_present(path: Path) -> bool:
    return (path / _COMPLETE_NAME).is_file()


def _required_repos() -> tuple[str, ...]:
    return tuple(repo for repo, _rev, _allow in _LOCAL_MODELS)


def models_status(app_data: str) -> dict:
    models: list[dict] = []
    total = 0
    for repo in _required_repos():
        path = _snapshot_path(app_data, repo)
        present = _model_present(path)
        size = _dir_size_bytes(path) if present else 0
        total += size
        models.append({"name": repo, "present": present, "sizeBytes": size})
    return {
        "ready": all(m["present"] for m in models),
        "models": models,
        "totalSizeBytes": total,
    }


def _require_complete(app_data: str, repo_id: str, label: str) -> Path:
    path = _snapshot_path(app_data, repo_id)
    if not _model_present(path):
        raise FileNotFoundError(
            f"{label} not downloaded (missing {_COMPLETE_NAME} at {path})"
        )
    return path


def _write_complete(app_data: str, repo_id: str) -> None:
    path = _snapshot_path(app_data, repo_id)
    path.mkdir(parents=True, exist_ok=True)
    (_complete_path(app_data, repo_id)).write_text("", encoding="utf-8")


def _clear_complete(app_data: str, repo_id: str) -> None:
    complete = _complete_path(app_data, repo_id)
    if complete.is_file():
        complete.unlink()


def _estimate_repo_bytes(repo_id: str, allow: tuple[str, ...], revision: str | None) -> int:
    from huggingface_hub import HfApi

    api = HfApi()
    info = api.model_info(repo_id, revision=revision, files_metadata=True)
    allow_set = set(allow)
    total = 0
    for sib in info.siblings or []:
        name = getattr(sib, "rfilename", None) or ""
        if name not in allow_set:
            continue
        size = getattr(sib, "size", None)
        if size is not None:
            total += int(size)
    return total


def estimate_download_bytes() -> int:
    total = 0
    for repo, rev, allow in _LOCAL_MODELS:
        total += _estimate_repo_bytes(repo, allow, rev)
    for repo, allow in _CACHE_DEPS:
        total += _estimate_repo_bytes(repo, allow, None)
    return total


def _download_local(
    app_data: str,
    repo_id: str,
    revision: str,
    allow: tuple[str, ...],
) -> None:
    from huggingface_hub import snapshot_download

    dest = _snapshot_path(app_data, repo_id)
    dest.mkdir(parents=True, exist_ok=True)
    complete = dest / _COMPLETE_NAME
    if complete.is_file():
        complete.unlink()
    snapshot_download(
        repo_id=repo_id,
        revision=revision,
        local_dir=str(dest),
        allow_patterns=list(allow),
    )


def _download_cache_dep(repo_id: str, allow: tuple[str, ...]) -> None:
    from huggingface_hub import snapshot_download

    snapshot_download(
        repo_id=repo_id,
        revision="main",
        allow_patterns=list(allow),
    )


def download_models(
    app_data: str,
    progress_cb: Callable[[dict[str, Any]], None] | None = None,
) -> dict:
    """Download pinned local models and HF cache deps; load each and write .complete."""
    os.environ["HF_HOME"] = str(hf_home(app_data))
    root = models_dir(app_data)
    root.mkdir(parents=True, exist_ok=True)
    hf_home(app_data).mkdir(parents=True, exist_ok=True)

    total = estimate_download_bytes()
    if progress_cb is not None:
        progress_cb({"totalBytes": total})

    for repo, rev, allow in _LOCAL_MODELS:
        _download_local(app_data, repo, rev, allow)
        if progress_cb is not None:
            progress_cb({"repo": repo})

    for repo, allow in _CACHE_DEPS:
        _download_cache_dep(repo, allow)
        if progress_cb is not None:
            progress_cb({"repo": repo})

    key = str(Path(app_data).resolve())
    _minilm_cache.pop(key, None)
    _hhem_cache.pop(key, None)
    _gliner_cache.pop(key, None)

    for repo, _rev, _allow in _LOCAL_MODELS:
        _clear_complete(app_data, repo)

    minilm = _load_minilm(app_data, verify_files=True)
    _ = _score_minilm_batch(minilm, "warm", ["warm passage"])
    _write_complete(app_data, MINILM_RERANKER_REPO)

    hhem = _load_hhem(app_data, verify_files=True)
    try:
        _ = hhem.predict([("warm premise", "warm hypothesis")])
    except Exception:
        pass
    _write_complete(app_data, HHEM_REPO)

    _ = load_gliner(app_data, verify_files=True)
    _write_complete(app_data, GLINER_REPO)

    return models_status(app_data)


def _ensure_torch_threads() -> None:
    global _torch_threads_set
    if _torch_threads_set:
        return
    import torch

    torch.set_num_threads(_TORCH_THREADS)
    _torch_threads_set = True


def sigmoid(x: float) -> float:
    if x >= 0:
        z = math.exp(-x)
        return 1.0 / (1.0 + z)
    z = math.exp(x)
    return z / (1.0 + z)


def _clip_passage(text: str, max_tokens: int = _RERANK_PASSAGE_TOKENS) -> str:
    words = (text or "").split()
    if len(words) <= max_tokens:
        return text or ""
    return " ".join(words[:max_tokens])


def _load_minilm(app_data: str, *, verify_files: bool = False) -> Any:
    key = str(Path(app_data).resolve())
    cached = _minilm_cache.get(key)
    if cached is not None:
        return cached

    _ensure_torch_threads()
    from sentence_transformers import CrossEncoder

    if verify_files:
        path = _snapshot_path(app_data, MINILM_RERANKER_REPO)
    else:
        path = _require_complete(app_data, MINILM_RERANKER_REPO, "MiniLM reranker")
    model = CrossEncoder(str(path), device="cpu", max_length=_RERANK_MAX_LENGTH)
    _minilm_cache[key] = model
    return model


def _load_hhem(app_data: str, *, verify_files: bool = False) -> Any:
    key = str(Path(app_data).resolve())
    cached = _hhem_cache.get(key)
    if cached is not None:
        return cached

    _ensure_torch_threads()
    import torch
    from transformers import AutoModelForSequenceClassification

    if verify_files:
        path = _snapshot_path(app_data, HHEM_REPO)
    else:
        path = _require_complete(app_data, HHEM_REPO, "HHEM model")
    model = AutoModelForSequenceClassification.from_pretrained(
        str(path),
        trust_remote_code=True,
        torch_dtype=torch.float32,
        local_files_only=not verify_files,
    )
    model.eval()
    model.to(torch.device("cpu"))
    _hhem_cache[key] = model
    return model


def _score_minilm_batch(model: Any, query: str, docs: list[str]) -> list[float]:
    pairs = [[query, _clip_passage(d)] for d in docs]
    raw = model.predict(pairs, batch_size=len(pairs), show_progress_bar=False)
    out: list[float] = []
    try:
        values = list(raw)
    except TypeError:
        values = [raw]
    for v in values:
        out.append(sigmoid(float(v)))
    return out


def rerank_scores(
    app_data: str,
    query: str,
    documents: list[str],
) -> list[float]:
    """Score documents; returns one score per input, preserving fused order for unscored."""
    result = rerank_with_budget(app_data, query, documents)
    return result["scores"]


def rerank_with_budget(
    app_data: str,
    query: str,
    documents: list[str],
) -> dict[str, Any]:
    n = len(documents)
    if n == 0:
        return {"scores": [], "scored": 0, "budgetHit": False, "rerankMs": 0}

    if mock_models_enabled():
        scores = [max(0.0, min(1.0, sigmoid(2.0 - i * 0.15))) for i in range(n)]
        return {"scores": scores, "scored": n, "budgetHit": False, "rerankMs": 0}

    minilm = _load_minilm(app_data)

    scores: list[float | None] = [None] * n
    started = time.perf_counter()
    scored = 0
    budget_hit = False
    for start in range(0, n, _RERANK_BATCH):
        elapsed = time.perf_counter() - started
        if elapsed >= _RERANK_BUDGET_S:
            budget_hit = True
            break
        batch = documents[start : start + _RERANK_BATCH]
        batch_scores = _score_minilm_batch(minilm, query, batch)
        for i, s in enumerate(batch_scores):
            scores[start + i] = float(s)
            scored += 1
        if time.perf_counter() - started >= _RERANK_BUDGET_S and start + _RERANK_BATCH < n:
            budget_hit = True
            break

    if budget_hit:
        log.info("rerank budget hit: %s of %s scored", scored, n)

    out: list[float] = []
    for i, s in enumerate(scores):
        if s is not None:
            out.append(s)
        else:
            out.append(-1.0 - (i * 1e-6))
    ms = int((time.perf_counter() - started) * 1000)
    return {"scores": out, "scored": scored, "budgetHit": budget_hit, "rerankMs": ms}


def hhem_scores_bounded(
    app_data: str,
    pairs: list[tuple[str, str]],
) -> list[float | None]:
    """Score pairs with a 4s wall budget; unscored entries are None."""
    if not pairs:
        return []
    if mock_models_enabled():
        return [0.9] * len(pairs)

    model = _load_hhem(app_data)
    out: list[float | None] = [None] * len(pairs)
    started = time.perf_counter()
    scored = 0
    for i, pair in enumerate(pairs):
        if time.perf_counter() - started >= _HHEM_BUDGET_S:
            log.info("hhem budget hit: %s of %s scored", scored, len(pairs))
            break
        try:
            raw = model.predict([pair])
            try:
                score = float(raw[0])  # type: ignore[index]
            except (TypeError, IndexError):
                score = float(raw)
            out[i] = score
            scored += 1
        except Exception:
            log.exception("hhem predict failed for pair %s", i)
            out[i] = float("nan")
    return out


def warm_checking_models(app_data: str) -> dict[str, Any]:
    """Load MiniLM + HHEM and run a tiny warm-up pair each."""
    if mock_models_enabled():
        return {"warmed": True, "loadMs": 0, "warmupMs": 0}

    t0 = time.perf_counter()
    _ensure_torch_threads()
    minilm = _load_minilm(app_data)
    hhem = _load_hhem(app_data)
    load_ms = int((time.perf_counter() - t0) * 1000)

    t1 = time.perf_counter()
    _ = _score_minilm_batch(minilm, "warm", ["warm passage"])
    try:
        _ = hhem.predict([("warm premise", "warm hypothesis")])
    except Exception:
        pass
    warm_ms = int((time.perf_counter() - t1) * 1000)
    log.info("checking models load %sms warm-up %sms", load_ms, warm_ms)
    return {"warmed": True, "loadMs": load_ms, "warmupMs": warm_ms}


def load_gliner(app_data: str, *, verify_files: bool = False) -> Any:
    key = str(Path(app_data).resolve())
    cached = _gliner_cache.get(key)
    if cached is not None:
        return cached

    from gliner import GLiNER

    if verify_files:
        path = _snapshot_path(app_data, GLINER_REPO)
    else:
        path = _require_complete(app_data, GLINER_REPO, "GLiNER model")
    # GLiNER.from_pretrained forwards kwargs to transformers; prefer offline when not verifying.
    try:
        model = GLiNER.from_pretrained(str(path), local_files_only=not verify_files)
    except TypeError:
        model = GLiNER.from_pretrained(str(path))
    _gliner_cache[key] = model
    return model


def unload_gliner() -> None:
    import gc

    _gliner_cache.clear()
    gc.collect()
    try:
        import torch

        if hasattr(torch, "mps") and hasattr(torch.mps, "empty_cache"):
            torch.mps.empty_cache()
        if hasattr(torch, "cuda") and torch.cuda.is_available():
            torch.cuda.empty_cache()
    except Exception:
        pass


def _mock_extract_spans(text: str, labels: list[str]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    if not text or not labels:
        return out
    label = next(
        (str(x) for x in labels if str(x).lower() != "date"),
        str(labels[0]),
    )
    for m in _MOCK_CAPITALISED_RE.finditer(text):
        out.append(
            {
                "label": label,
                "text": m.group(0),
                "start": m.start(),
                "end": m.end(),
                "score": 0.99,
            }
        )
    if any(str(x).lower() == "date" for x in labels):
        for m in _MOCK_DATE_RE.finditer(text):
            if any(
                s["start"] <= m.start() and s["end"] >= m.end() and s["label"] == "date"
                for s in out
            ):
                continue
            out.append(
                {
                    "label": "date",
                    "text": m.group(0),
                    "start": m.start(),
                    "end": m.end(),
                    "score": 0.95,
                }
            )
    out.sort(key=lambda s: (int(s["start"]), int(s["end"])))
    return out


def _normalize_span_item(item: Any) -> dict[str, Any]:
    if isinstance(item, dict):
        return {
            "label": str(item.get("label") or ""),
            "text": str(item.get("text") or ""),
            "start": int(item.get("start") or 0),
            "end": int(item.get("end") or 0),
            "score": float(item.get("score") or 0.0),
        }
    return {
        "label": str(getattr(item, "label", "") or ""),
        "text": str(getattr(item, "text", "") or ""),
        "start": int(getattr(item, "start", 0) or 0),
        "end": int(getattr(item, "end", 0) or 0),
        "score": float(getattr(item, "score", 0.0) or 0.0),
    }


def extract_spans_batch(
    app_data: str,
    texts: list[str],
    labels: list[str],
    threshold: float = 0.5,
) -> list[list[dict[str, Any]]]:
    if not texts or not labels:
        return [[] for _ in texts]
    if mock_models_enabled():
        return [_mock_extract_spans(t, labels) for t in texts]

    model = load_gliner(app_data)
    raw_batches = model.inference(
        texts,
        labels,
        threshold=threshold,
        batch_size=8,
    )
    out: list[list[dict[str, Any]]] = []
    for raw in raw_batches or []:
        spans: list[dict[str, Any]] = []
        for item in raw or []:
            spans.append(_normalize_span_item(item))
        out.append(spans)
    while len(out) < len(texts):
        out.append([])
    return out[: len(texts)]
