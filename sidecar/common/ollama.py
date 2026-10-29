"""HTTP helpers for local Ollama."""

from __future__ import annotations

import os
from typing import Any

import httpx

def _normalize_host(host: str) -> str:
    if host.startswith("http://") or host.startswith("https://"):
        return host
    return f"http://{host}"


DEFAULT_HOST = _normalize_host(os.environ.get("OLLAMA_HOST", "http://127.0.0.1:11434"))

# Keep in sync with Rust `CHAT_CONTEXT_TOKENS` in ollama_stream.rs.
CHAT_CONTEXT_TOKENS = 8192


def _client() -> httpx.Client:
    return httpx.Client(base_url=DEFAULT_HOST, timeout=120.0)


def embed_text(model: str, text: str) -> list[float]:
    payload = {"model": model, "prompt": text}
    with _client() as c:
        r = c.post("/api/embeddings", json=payload)
        r.raise_for_status()
        data = r.json()
    return data.get("embedding", [])


def warm_model(model: str) -> dict[str, Any]:
    """Pre-load model weights; uses generate with keep_alive when supported."""
    payload = {
        "model": model,
        "prompt": " ",
        "keep_alive": -1,
        "stream": False,
        "options": {"num_ctx": CHAT_CONTEXT_TOKENS},
    }
    with _client() as c:
        r = c.post("/api/generate", json=payload)
        r.raise_for_status()
        return r.json()
