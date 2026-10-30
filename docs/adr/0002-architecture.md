# ADR 002: Process boundaries and stack

## Status

Accepted

## Context

The app needs a desktop window, local storage, document parsing, embeddings, graph extraction, and chat against Ollama without shipping a separate graph server or cloud pipeline.

## Decision

Tauri and Rust own the window, registered invoke commands, vault paths, SQLite, import jobs, file watching, and the sidecar process. The Python sidecar talks over line-delimited JSON-RPC on stdin and stdout. It parses and chunks documents, embeds with sqlite-vec, extracts a LightRAG-style graph into DuckDB, and prepares chat retrieval. Ollama runs as a separate local service for chat and embeddings. The stack stays on SQLite with sqlite-vec, DuckDB adjacency tables, and uv for the Python environment.

## Consequences

Packaging must ship a reproducible Python runtime with the app. Security expectations for the webview and network are covered in ADR 004. Graph storage ownership is covered in ADR 006.
