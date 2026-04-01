# ADR 002b: Revised MVP runtime architecture (process and data boundaries)

## Status

Accepted

## Context

Cognito is a local-first desktop workspace. The revised MVP combines a **Tauri (Rust)** shell with a **Python sidecar** for ingestion and hybrid retrieval, **SQLite + sqlite-vec**, **DuckDB** for graph adjacency, and **Ollama** on localhost for inference.

## Decision

1. **Tauri / Rust** is the only component that exposes **commands to the UI** (`invoke`). The command surface is enumerated in `src-tauri/src/lib.rs` (`tauri::generate_handler![...]`). File access from the UI goes through these commands or Tauri-approved APIs (e.g. dialog), not arbitrary paths from the frontend.
2. **Python sidecar** runs as a **child process** with JSON-RPC **line-delimited stdin/stdout**. No separate HTTP server for document processing; optional localhost calls (e.g. Ollama) are confined to the sidecar and user-operated services.
3. **Vault and databases** live under the app data directory (Rust); the sidecar receives **paths** from Rust over RPC and must not invent alternate roots.
4. **Ollama** is **out-of-process** on the user machine; models are pulled and run locally, not shipped as Cognito bundle assets by default.

## Consequences

- Packaging must ship a **reproducible Python environment** (uv + `.venv`) beside the app; see ADR on sidecar bundling.
- Security and privacy expectations are detailed in `docs/adr/0004-security-privacy-boundaries.md`.

## Related

- `docs/adr/0002-revised-mvp-stack.md` — technology choices (LightRAG, DuckDB, uv).
- `docs/adr/0003-sidecar-bundling-with-uv.md` — shipping the sidecar in release builds.
