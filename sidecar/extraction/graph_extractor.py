"""Call LightRAG `extract_entities` one chunk at a time for stable chunk ↔ evidence mapping."""

from __future__ import annotations

from typing import Any

from lightrag.base import TextChunkSchema
from lightrag.operate import extract_entities

from extraction.extractor_config import ExtractorConfig, build_lightrag_global_config


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
