-- Drop legacy BLOB embedding table; sidecar recreates sqlite-vec vec0 virtual table on first ingest.

DROP TABLE IF EXISTS chunk_embeddings;
