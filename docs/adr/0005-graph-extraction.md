# ADR 005: LightRAG-style graph extraction

## Status

Accepted

## Context

Chat benefits from entity and relation hints drawn from chunked text. A full cloud GraphRAG pipeline is heavier than a desktop app needs.

## Decision

Extraction runs in the Python sidecar with LightRAG-style prompts (`lightrag-hku`), processing chunks sequentially and merging nodes and edges into DuckDB. A background worker in Rust schedules per-document work after indexing so chat can start once embeddings exist.

## Consequences

Graph quality depends on the chat model and chunk text. Per-document failures are recorded and do not wipe the rest of the library. Vector-only retrieval still works when extraction fails.
