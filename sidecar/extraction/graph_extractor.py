"""Call LightRAG `extract_entities` one chunk at a time for stable chunk ↔ evidence mapping."""

from __future__ import annotations

import os
from typing import Any

from lightrag.base import TextChunkSchema
from lightrag.operate import extract_entities

from extraction.extractor_config import ExtractorConfig, build_lightrag_global_config

MOCK_EXTRACT_ENV = "COGNITO_MOCK_EXTRACT"


def mock_extract_enabled() -> bool:
    return os.environ.get(MOCK_EXTRACT_ENV) == "1"


def _mock_chunk_maps() -> tuple[dict[str, Any], dict[tuple[str, str], Any]]:
    maybe_nodes = {
        "Mock Alpha": [
            {
                "entity_name": "Mock Alpha",
                "entity_type": "concept",
                "description": "mock entity",
            }
        ],
        "Mock Beta": [
            {
                "entity_name": "Mock Beta",
                "entity_type": "concept",
                "description": "mock entity",
            }
        ],
    }
    maybe_edges = {
        ("Mock Alpha", "Mock Beta"): [
            {
                "src_id": "Mock Alpha",
                "tgt_id": "Mock Beta",
                "weight": 0.9,
                "description": "relates to",
            }
        ]
    }
    return maybe_nodes, maybe_edges


async def extract_chunks_sequential(
    document_id: str,
    file_path: str,
    chunks: list[tuple[str, str, int, int]],
    cfg: ExtractorConfig,
) -> list[tuple[str, dict[str, Any], dict[tuple[str, str], Any]]]:
    """
    Args:
        chunks: list of (chunk_id, text, ordinal, token_estimate)

    Returns:
        List of (chunk_id, maybe_nodes, maybe_edges) in the same order as input.
    """
    if mock_extract_enabled():
        maybe_nodes, maybe_edges = _mock_chunk_maps()
        return [(chunk_id, maybe_nodes, maybe_edges) for chunk_id, _text, _ord, _tok in chunks]

    gc = build_lightrag_global_config(cfg)
    out: list[tuple[str, dict[str, Any], dict[tuple[str, str], Any]]] = []
    for chunk_id, text, ordinal, tokens in chunks:
        tc: TextChunkSchema = {
            "tokens": max(1, tokens),
            "content": text,
            "full_doc_id": document_id,
            "chunk_order_index": ordinal,
            "file_path": file_path,
        }
        batch = {chunk_id: tc}
        chunk_results = await extract_entities(batch, gc, None, None, None, None)
        if not chunk_results:
            out.append((chunk_id, {}, {}))
            continue
        maybe_nodes, maybe_edges = chunk_results[0]
        out.append((chunk_id, maybe_nodes, maybe_edges))
    return out
