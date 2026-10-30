from ingestion.chunking import estimate_tokens


def test_estimate_tokens_positive():
    assert estimate_tokens("hello world") >= 1
