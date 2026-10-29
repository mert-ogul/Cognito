"""HTTP helpers for local Ollama."""

from __future__ import annotations

import json
import os
from typing import Any, Iterator

import httpx

def _normalize_host(host: str) -> str:
    if host.startswith("http://") or host.startswith("https://"):
        return host
    return f"http://{host}"


DEFAULT_HOST = _normalize_host(os.environ.get("OLLAMA_HOST", "http://127.0.0.1:11434"))


def _client() -> httpx.Client:
    return httpx.Client(base_url=DEFAULT_HOST, timeout=120.0)


def embed_text(model: str, text: str) -> list[float]:
    payload = {"model": model, "prompt": text}
    with _client() as c:
        r = c.post("/api/embeddings", json=payload)
        r.raise_for_status()
        data = r.json()
    return data.get("embedding", [])


def chat_complete(model: str, messages: list[dict[str, str]], stream: bool = False) -> str:
    payload: dict[str, Any] = {"model": model, "messages": messages, "stream": stream}
    with _client() as c:
        r = c.post("/api/chat", json=payload)
        r.raise_for_status()
        data = r.json()
    return data.get("message", {}).get("content", "") or ""


def chat_stream(model: str, messages: list[dict[str, str]]) -> Iterator[str]:
    payload = {"model": model, "messages": messages, "stream": True}
    with _client() as c:
        with c.stream("POST", "/api/chat", json=payload) as resp:
            resp.raise_for_status()
            for line in resp.iter_lines():
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue
                msg = obj.get("message", {})
                t = msg.get("content")
                if t:
                    yield t


def warm_model(model: str) -> dict[str, Any]:
    """Pre-load model weights; uses generate with keep_alive when supported."""
    payload = {"model": model, "prompt": " ", "keep_alive": -1, "stream": False}
    with _client() as c:
        r = c.post("/api/generate", json=payload)
        r.raise_for_status()
        return r.json()


def ollama_health() -> bool:
    try:
        with _client() as c:
            r = c.get("/api/tags")
            return r.status_code == 200
    except OSError:
        return False
