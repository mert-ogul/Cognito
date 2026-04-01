"""LightRAG extraction settings — small surface so callers avoid vendor imports."""

from __future__ import annotations

from dataclasses import dataclass

from lightrag.constants import DEFAULT_ENTITY_TYPES, DEFAULT_SUMMARY_LANGUAGE
from lightrag.llm.ollama import ollama_model_complete


@dataclass(frozen=True)
class ExtractorConfig:
    chat_model: str
    entity_extract_max_gleaning: int = 0
    llm_max_async: int = 1


class _HashingKv:
    __slots__ = ("global_config",)

    def __init__(self, model_name: str) -> None:
        self.global_config = {"llm_model_name": model_name}


def build_lightrag_global_config(cfg: ExtractorConfig) -> dict:
    """Minimal `global_config` dict for `lightrag.operate.extract_entities` (no full LightRAG instance)."""

    chat = cfg.chat_model

    async def llm_model_func(prompt, system_prompt=None, history_messages=None, **kwargs):
        kwargs["hashing_kv"] = _HashingKv(chat)
        return await ollama_model_complete(
            prompt,
            system_prompt=system_prompt,
            history_messages=history_messages or [],
            **kwargs,
        )

    return {
        "llm_model_func": llm_model_func,
        "entity_extract_max_gleaning": cfg.entity_extract_max_gleaning,
        "addon_params": {
            "language": DEFAULT_SUMMARY_LANGUAGE,
            "entity_types": list(DEFAULT_ENTITY_TYPES),
        },
        "llm_model_max_async": cfg.llm_max_async,
    }
