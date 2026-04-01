# ADR 002: Revised MVP stack (LightRAG, DuckDB, uv sidecar)

## Status

Accepted

## Context

An earlier plan considered alternate graph engines, packaging, and query routers. The product constitution mandates a **revised MVP** that stays local-first and ships on a predictable timeline.

## Decision

1. **LightRAG-style extraction** (not Microsoft GraphRAG) for entity/relation signals from chunks, implemented in the **Python sidecar** where iteration and model I/O already live.
2. **DuckDB** for **adjacency** (`graph_nodes`, `graph_edges`) — fast local analytics, simple SQL, small footprint; avoids shipping a separate graph server.
3. **uv + bundled venv** for the sidecar instead of PyInstaller — faster iteration, smaller conceptual surface, aligns with developer workflows.
4. **Rust** owns process lifecycle, SQLite vault metadata, and **DuckDB file bootstrap** (schema applied on open); heavy graph writes stay in the sidecar to keep one writer path during ingestion.
5. **Ollama** remains the local inference and embedding provider; models are user-configurable with defaults `phi4-mini` / `nomic-embed-text`.

## Consequences

- Graph logic is split by responsibility: **Rust** opens and migrates databases; **Python** performs ingestion-time graph updates and hybrid retrieval math.
- Packaging must still ship a working Python environment (bundled `.venv` or equivalent) beside the Tauri binary.

## Related

- [Runtime architecture (process boundaries)](./0002-revised-mvp-architecture.md)
- [Sidecar bundling with uv](./0003-sidecar-bundling-with-uv.md)
