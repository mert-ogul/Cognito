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
QWEN_RERANKER_REPO = "Qwen/Qwen3-Reranker-0.6B"
# Back-compat alias used by older call sites / eval scripts.
RERANKER_REPO = MINILM_RERANKER_REPO
HHEM_REPO = "vectara/hallucination_evaluation_model"
GLINER_REPO = "urchade/gliner_multi-v2.1"

_MOCK_MODELS_ENV = "COGNITO_MOCK_MODELS"

_RERANK_BATCH = 8
_RERANK_BUDGET_S = 3.0
_RERANK_MAX_LENGTH = 384
_RERANK_PASSAGE_TOKENS = 300
_HHEM_BUDGET_S = 4.0
_TORCH_THREADS = 4

_RERANK_SYSTEM = (
    'Judge whether the Document meets the requirements based on the Query and '
    'the Instruct provided. Note that the answer can only be "yes" or "no".'
)
_RERANK_INSTRUCT = "Given a question, find passages in the user's documents that answer it"
_RERANK_PREFIX = (
    "<|im_start|>system\n"
    f"{_RERANK_SYSTEM}"
    "<|im_end|>\n"
    "<|im_start|>user\n"
)
_RERANK_SUFFIX = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"

# Lazy caches keyed by resolved app_data path.
_minilm_cache: dict[str, Any] = {}
_qwen_cache: dict[str, tuple[Any, Any, int, int, Any]] = {}
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
_MOCK_CORA_RE = re.compile(r"\bCora(?:\s+dataset)?\b", re.IGNORECASE)
_MOCK_GCN_RE = re.compile(
    r"\bGraph Convolutional Networks\b|\bGCN\b",
    re.IGNORECASE,
)


def models_dir(app_data: str) -> Path:
    return Path(app_data) / "models"


def mock_models_enabled() -> bool:
    return os.environ.get(_MOCK_MODELS_ENV) == "1"


def _repo_folder(repo_id: str) -> str:
    return repo_id.replace("/", "--")


def _snapshot_path(app_data: str, repo_id: str) -> Path:
    return models_dir(app_data) / _repo_folder(repo_id)


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
    # GLiNER ships gliner_config.json instead of Hugging Face config.json.
    markers = ("config.json", "gliner_config.json")
    if any((path / name).is_file() for name in markers):
        return True
    if not path.is_dir():
        return False
    for root, _dirs, files in os.walk(path):
        if any(name in files for name in markers):
            return True
    return False


def _required_repos(thorough_reranking: bool) -> tuple[str, ...]:
    repos = [MINILM_RERANKER_REPO, HHEM_REPO]
    if thorough_reranking:
        repos.append(QWEN_RERANKER_REPO)
    return tuple(repos)


def models_status(app_data: str, thorough_reranking: bool = False) -> dict:
    models: list[dict] = []
    total = 0
    for repo in _required_repos(thorough_reranking):
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


def download_models(
    app_data: str,
    progress_cb: Callable[[str], None] | None = None,
    thorough_reranking: bool = False,
) -> dict:
    from huggingface_hub import snapshot_download

    def log_line(msg: str) -> None:
        if progress_cb is not None:
            progress_cb(msg)

    root = models_dir(app_data)
    root.mkdir(parents=True, exist_ok=True)
    for repo in _required_repos(thorough_reranking):
        dest = _snapshot_path(app_data, repo)
        log_line(f"Downloading {repo}...")
        snapshot_download(repo_id=repo, local_dir=str(dest))
        log_line(f"Ready {repo}")
    return models_status(app_data, thorough_reranking=thorough_reranking)


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


def _load_minilm(app_data: str) -> Any:
    key = str(Path(app_data).resolve())
    cached = _minilm_cache.get(key)
    if cached is not None:
        return cached

    _ensure_torch_threads()
    from sentence_transformers import CrossEncoder

    path = _snapshot_path(app_data, MINILM_RERANKER_REPO)
    if not _model_present(path):
        raise FileNotFoundError(f"MiniLM reranker missing at {path}")

    model = CrossEncoder(str(path), device="cpu", max_length=_RERANK_MAX_LENGTH)
    _minilm_cache[key] = model
    return model


def _mps_available() -> bool:
    import torch

    return bool(hasattr(torch.backends, "mps") and torch.backends.mps.is_available())


def _load_qwen_reranker(app_data: str) -> tuple[Any, Any, int, int, Any]:
    """Load Qwen3-Reranker on MPS only. Raises if MPS is unavailable or warm-up fails."""
    if not _mps_available():
        raise RuntimeError("Qwen reranker requires Apple MPS; refusing CPU")

    key = str(Path(app_data).resolve())
    cached = _qwen_cache.get(key)
    if cached is not None:
        return cached

    _ensure_torch_threads()
    import torch
    from transformers import AutoModelForCausalLM, AutoTokenizer

    path = _snapshot_path(app_data, QWEN_RERANKER_REPO)
    if not _model_present(path):
        raise FileNotFoundError(f"Qwen reranker missing at {path}")

    device = torch.device("mps")
    tokenizer = AutoTokenizer.from_pretrained(
        str(path), trust_remote_code=True, padding_side="left"
    )
    model = AutoModelForCausalLM.from_pretrained(
        str(path),
        trust_remote_code=True,
        torch_dtype=torch.float16,
    )
    model.eval()
    model.to(device)
    yes_id = tokenizer.convert_tokens_to_ids("yes")
    no_id = tokenizer.convert_tokens_to_ids("no")

    # Warm-up must succeed on MPS before we keep the model.
    try:
        warm = tokenizer(
            ["warm"],
            return_tensors="pt",
            padding=True,
            truncation=True,
            max_length=32,
        )
        warm = {k: v.to(device) for k, v in warm.items()}
        with torch.no_grad():
            _ = model(**warm).logits
    except Exception as e:
        del model
        raise RuntimeError(f"Qwen MPS warm-up failed: {e}") from e

    packed = (tokenizer, model, yes_id, no_id, device)
    _qwen_cache[key] = packed
    return packed


def _load_hhem(app_data: str) -> Any:
    key = str(Path(app_data).resolve())
    cached = _hhem_cache.get(key)
    if cached is not None:
        return cached

    _ensure_torch_threads()
    import torch
    from transformers import AutoModelForSequenceClassification

    path = _snapshot_path(app_data, HHEM_REPO)
    if not _model_present(path):
        raise FileNotFoundError(f"HHEM model missing at {path}")

    model = AutoModelForSequenceClassification.from_pretrained(
        str(path),
        trust_remote_code=True,
        torch_dtype=torch.float32,
    )
    model.eval()
    model.to(torch.device("cpu"))
    _hhem_cache[key] = model
    return model


def _format_qwen_pair(query: str, doc: str) -> str:
    return (
        f"<Instruct>: {_RERANK_INSTRUCT}\n"
        f"<Query>: {query}\n"
        f"<Document>: {doc}"
    )


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


def _score_qwen_batch(
    packed: tuple[Any, Any, int, int, Any],
    query: str,
    docs: list[str],
) -> list[float]:
    import torch

    tokenizer, model, yes_id, no_id, device = packed
    prefix_tokens = tokenizer.encode(_RERANK_PREFIX, add_special_tokens=False)
    suffix_tokens = tokenizer.encode(_RERANK_SUFFIX, add_special_tokens=False)
    pairs = [_format_qwen_pair(query, _clip_passage(d)) for d in docs]
    budget = _RERANK_MAX_LENGTH - len(prefix_tokens) - len(suffix_tokens)
    tokenized = tokenizer(
        pairs,
        padding=False,
        truncation="longest_first",
        return_attention_mask=False,
        max_length=max(1, budget),
    )
    for i, ids in enumerate(tokenized["input_ids"]):
        tokenized["input_ids"][i] = prefix_tokens + ids + suffix_tokens
    inputs = tokenizer.pad(
        tokenized,
        padding=True,
        return_tensors="pt",
        max_length=_RERANK_MAX_LENGTH,
    )
    inputs = {k: v.to(device) for k, v in inputs.items()}
    with torch.no_grad():
        logits = model(**inputs).logits[:, -1, :]
        stacked = torch.stack([logits[:, no_id], logits[:, yes_id]], dim=1)
        probs = torch.nn.functional.softmax(stacked, dim=1)
        return probs[:, 1].detach().cpu().tolist()


def rerank_scores(
    app_data: str,
    query: str,
    documents: list[str],
    *,
    thorough_reranking: bool = False,
) -> list[float]:
    """Score documents; returns one score per input, preserving fused order for unscored."""
    result = rerank_with_budget(
        app_data,
        query,
        documents,
        thorough_reranking=thorough_reranking,
    )
    return result["scores"]


def rerank_with_budget(
    app_data: str,
    query: str,
    documents: list[str],
    *,
    thorough_reranking: bool = False,
) -> dict[str, Any]:
    n = len(documents)
    if n == 0:
        return {"scores": [], "scored": 0, "budgetHit": False, "rerankMs": 0}

    if mock_models_enabled():
        scores = [max(0.0, min(1.0, sigmoid(2.0 - i * 0.15))) for i in range(n)]
        return {"scores": scores, "scored": n, "budgetHit": False, "rerankMs": 0}

    use_qwen = False
    qwen_packed = None
    minilm = None
    if thorough_reranking:
        try:
            qwen_packed = _load_qwen_reranker(app_data)
            use_qwen = True
        except Exception as e:
            log.warning("Thorough reranker unavailable (%s); using MiniLM", e)

    if not use_qwen:
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
        if use_qwen and qwen_packed is not None:
            batch_scores = _score_qwen_batch(qwen_packed, query, batch)
        else:
            assert minilm is not None
            batch_scores = _score_minilm_batch(minilm, query, batch)
        for i, s in enumerate(batch_scores):
            scores[start + i] = float(s)
            scored += 1
        if time.perf_counter() - started >= _RERANK_BUDGET_S and start + _RERANK_BATCH < n:
            budget_hit = True
            break

    if budget_hit:
        log.info("rerank budget hit: %s of %s scored", scored, n)

    # Unscored keep fused order after scored ones via a descending floor.
    out: list[float] = []
    for i, s in enumerate(scores):
        if s is not None:
            out.append(s)
        else:
            out.append(-1.0 - (i * 1e-6))
    ms = int((time.perf_counter() - started) * 1000)
    return {"scores": out, "scored": scored, "budgetHit": budget_hit, "rerankMs": ms}


def hhem_scores(
    app_data: str,
    pairs: list[tuple[str, str]],
) -> list[float | None]:
    """Return per-pair scores; None means the pair was not scored within budget."""
    if not pairs:
        return []
    if mock_models_enabled():
        return [0.9] * len(pairs)

    model = _load_hhem(app_data)
    started = time.perf_counter()
    try:
        raw = model.predict(list(pairs))
    except Exception:
        # Older HHEM builds may not expose predict; fall back to one-shot if fast enough.
        return [None] * len(pairs)

    elapsed = time.perf_counter() - started
    try:
        values = [float(x) for x in raw]
    except TypeError:
        values = [float(raw)]

    if elapsed <= _HHEM_BUDGET_S:
        return values

    # Whole-batch overran: mark all unchecked (cannot partially stop native predict).
    log.info("hhem budget hit: 0 of %s scored", len(pairs))
    return [None] * len(pairs)


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
    # Score one-by-one so we can stop at the budget (HHEM predict is otherwise all-or-nothing).
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
            out[i] = None
    return out


def warm_checking_models(
    app_data: str,
    thorough_reranking: bool = False,
) -> dict[str, Any]:
    """Load MiniLM + HHEM (and Qwen if thorough) and run a tiny warm-up pair each."""
    if mock_models_enabled():
        return {"warmed": True, "loadMs": 0, "warmupMs": 0}

    t0 = time.perf_counter()
    _ensure_torch_threads()
    minilm = _load_minilm(app_data)
    hhem = _load_hhem(app_data)
    qwen_ok = False
    if thorough_reranking:
        try:
            _load_qwen_reranker(app_data)
            qwen_ok = True
        except Exception as e:
            log.warning("Thorough reranker warm skipped: %s", e)
    load_ms = int((time.perf_counter() - t0) * 1000)

    t1 = time.perf_counter()
    _ = _score_minilm_batch(minilm, "warm", ["warm passage"])
    try:
        _ = hhem.predict([("warm premise", "warm hypothesis")])
    except Exception:
        pass
    if qwen_ok:
        pass  # warm already done inside _load_qwen_reranker
    warm_ms = int((time.perf_counter() - t1) * 1000)
    log.info(
        "checking models load %sms warm-up %sms (thorough=%s)",
        load_ms,
        warm_ms,
        thorough_reranking and qwen_ok,
    )
    return {"warmed": True, "loadMs": load_ms, "warmupMs": warm_ms}


def load_gliner(app_data: str) -> Any:
    key = str(Path(app_data).resolve())
    cached = _gliner_cache.get(key)
    if cached is not None:
        return cached

    from gliner import GLiNER
    from huggingface_hub import snapshot_download

    path = _snapshot_path(app_data, GLINER_REPO)
    if not _model_present(path):
        path.parent.mkdir(parents=True, exist_ok=True)
        snapshot_download(repo_id=GLINER_REPO, local_dir=str(path))

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


def _pick_mock_label(labels: list[str], preferred: list[str]) -> str:
    lower = {str(x).lower(): str(x) for x in labels}
    for p in preferred:
        if p.lower() in lower:
            return lower[p.lower()]
    return str(labels[0]) if labels else "entity"


def _mock_extract_spans(text: str, labels: list[str]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    if not text:
        return out
    for m in _MOCK_CORA_RE.finditer(text):
        span = m.group(0)
        label = _pick_mock_label(
            labels, ["dataset", "organization", "model", "algorithm"]
        )
        out.append(
            {
                "label": label,
                "text": span,
                "start": m.start(),
                "end": m.end(),
                "score": 0.99,
            }
        )
    for m in _MOCK_GCN_RE.finditer(text):
        span = m.group(0)
        label = _pick_mock_label(
            labels, ["model", "algorithm", "method", "organization"]
        )
        out.append(
            {
                "label": label,
                "text": span,
                "start": m.start(),
                "end": m.end(),
                "score": 0.98,
            }
        )
    if any(str(x).lower() == "date" for x in labels):
        for m in _MOCK_DATE_RE.finditer(text):
            span = m.group(0)
            if any(
                s["start"] <= m.start() and s["end"] >= m.end() and s["label"] == "date"
                for s in out
            ):
                continue
            out.append(
                {
                    "label": "date",
                    "text": span,
                    "start": m.start(),
                    "end": m.end(),
                    "score": 0.95,
                }
            )
    out.sort(key=lambda s: (int(s["start"]), int(s["end"])))
    return out


def extract_spans(
    app_data: str,
    text: str,
    labels: list[str],
    threshold: float = 0.5,
) -> list[dict[str, Any]]:
    if not text or not labels:
        return []
    if mock_models_enabled():
        return _mock_extract_spans(text, labels)

    model = load_gliner(app_data)
    raw = model.predict_entities(text, labels, threshold=threshold)
    out: list[dict[str, Any]] = []
    for item in raw or []:
        if isinstance(item, dict):
            out.append(
                {
                    "label": str(item.get("label") or ""),
                    "text": str(item.get("text") or ""),
                    "start": int(item.get("start") or 0),
                    "end": int(item.get("end") or 0),
                    "score": float(item.get("score") or 0.0),
                }
            )
        else:
            out.append(
                {
                    "label": str(getattr(item, "label", "") or ""),
                    "text": str(getattr(item, "text", "") or ""),
                    "start": int(getattr(item, "start", 0) or 0),
                    "end": int(getattr(item, "end", 0) or 0),
                    "score": float(getattr(item, "score", 0.0) or 0.0),
                }
            )
    return out
