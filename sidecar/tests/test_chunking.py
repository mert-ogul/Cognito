from ingestion.chunking import chunk_text, estimate_tokens


def test_estimate_tokens_positive():
    assert estimate_tokens("hello world") >= 1


def test_chunk_text_splits():
    text = "x" * 5000
    chunks = chunk_text(text, max_chars=1000, overlap=100)
    assert len(chunks) >= 2
