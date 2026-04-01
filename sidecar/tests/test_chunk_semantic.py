"""Semantic chunking shape."""

from ingestion.chunking import chunk_semantic, estimate_tokens


def test_chunk_semantic_multiple_chunks() -> None:
    text = "word " * 3000
    chunks = chunk_semantic(text, "markdown")
    assert len(chunks) >= 2
    for c in chunks:
        assert estimate_tokens(c.text) >= 1
