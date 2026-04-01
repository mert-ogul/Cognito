"""Hybrid query helpers and legacy non-streaming `query_hybrid` (uses chat pipeline)."""

from __future__ import annotations

from common.ollama import chat_complete
from query.chat_pipeline import prepare_chat_retrieval


def query_hybrid(
    sqlite_path: str,
    duck_path: str,
    query: str,
    chat_model: str,
    embed_model: str,
) -> dict:
    """Non-streaming answer for RPC / tests; same retrieval as streaming chat."""
    prep = prepare_chat_retrieval(sqlite_path, duck_path, query, embed_model)
    messages = prep["messages"]
    answer = chat_complete(
        chat_model,
        messages,
        stream=False,
    )
    return {
        "answer": answer.strip(),
        "citations": prep["citations"],
        "retrievalMode": prep["retrievalMode"],
        "graphAugmented": prep.get("graphAugmented", False),
    }


def health(embed_model: str, chat_model: str) -> dict:
    from common.ollama import ollama_health

    ok = ollama_health()
    return {
        "ollama": ok,
        "ollama_model_ready": ok,
        "embed_model": embed_model,
        "chat_model": chat_model,
    }
