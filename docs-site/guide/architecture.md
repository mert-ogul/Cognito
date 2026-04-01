# Architecture (overview)

Cognito is a **Tauri 2** desktop app with a **Rust** backend and **React** frontend.

- **Vault**: SQLite stores document metadata, chunk text, and sqlite-vec embeddings.
- **Sidecar**: Python (`uv` + bundled `.venv`) handles parsing, chunking, embedding RPCs, LightRAG-style graph extraction, and hybrid retrieval JSON-RPC methods.
- **Graph**: DuckDB adjacency tables (`graph_nodes`, `graph_edges`) owned by the sidecar; Rust applies schema on open.
- **Inference**: **Ollama** for chat and embeddings (user-configurable models).

See repository `docs/adr/` for decisions and trade-offs.
