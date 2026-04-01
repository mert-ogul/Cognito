# ADR 006: LightRAG-style graph extraction

## Status

Accepted

## Context

The MVP needs **entity and relation signals** from chunked text to populate a local graph. Full Microsoft GraphRAG pipelines are heavier than needed for a desktop MVP.

## Decision

Use **LightRAG-style extraction** (via the `lightrag-hku` dependency in the sidecar): sequential chunk processing with structured prompts, merging into DuckDB-backed **nodes and edges**. Extraction runs in the **Python sidecar** next to parsing and embedding, where async I/O and model calls already live.

## Consequences

- Graph quality depends on the configured **chat model** and chunk text; failures are surfaced per document and ingestion continues where possible.
- The product name “LightRAG” refers to the approach and library; it is not a cloud service.

## Related

- [Revised MVP stack](./0002-revised-mvp-stack.md)
- [DuckDB adjacency graph](./0007-duckdb-adjacency-graph.md)
