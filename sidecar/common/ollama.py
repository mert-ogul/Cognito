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

_EMBED_BATCH = 32

_QWEN3_QUERY_PREFIX = (
    "Instruct: Given a question, retrieve passages from the user's documents "
    "that answer it\nQuery: "
)


def embed_prefixes(model: str) -> tuple[str, str]:
    """Return (query_prefix, document_prefix) for the embedding model."""
    name = (model or "").split(":", 1)[0].strip().lower()
    if name == "nomic-embed-text":
        return ("search_query: ", "search_document: ")
    if name == "mxbai-embed-large":
        return ("Represent this sentence for searching relevant passages: ", "")
    if name.startswith("qwen3-embedding"):
        return (_QWEN3_QUERY_PREFIX, "")
    return ("", "")


def _client() -> httpx.Client:
    return httpx.Client(base_url=DEFAULT_HOST, timeout=120.0)


def embed_texts(model: str, texts: list[str]) -> list[list[float]]:
    if not texts:
        return []
    out: list[list[float]] = []
    with _client() as c:
        for start in range(0, len(texts), _EMBED_BATCH):
            batch = texts[start : start + _EMBED_BATCH]
            r = c.post("/api/embed", json={"model": model, "input": batch})
            r.raise_for_status()
            data = r.json()
            embeddings = data.get("embeddings") or []
            out.extend(embeddings)
    return out


def embed_text(model: str, text: str) -> list[float]:
    return embed_texts(model, [text])[0]


def warm_model(model: str) -> dict[str, Any]:
    """Pre-load model weights; uses generate with keep_alive when supported."""
    payload = {
        "model": model,
        "prompt": " ",
        "keep_alive": -1,
        "stream": False,
        "think": False,
        "options": {"num_ctx": CHAT_CONTEXT_TOKENS},
    }
    with _client() as c:
        r = c.post("/api/generate", json=payload)
        r.raise_for_status()
        return r.json()


def chat(
    model: str,
    messages: list[dict[str, str]],
    *,
    think: bool = False,
) -> str:
    """One-shot /api/chat; returns assistant message content."""
    payload: dict[str, Any] = {
        "model": model,
        "messages": messages,
        "stream": False,
        "think": think,
        "options": {"num_ctx": CHAT_CONTEXT_TOKENS},
    }
    with _client() as c:
        r = c.post("/api/chat", json=payload)
        r.raise_for_status()
        data = r.json()
    msg = data.get("message") or {}
    return str(msg.get("content") or "")
