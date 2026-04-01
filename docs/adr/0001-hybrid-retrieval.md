# ADR 001: Deterministic hybrid retrieval

## Status

Accepted

## Context

Cognito MVP requires grounded answers with citations over a local vault, with optional graph augmentation. Adaptive routing is out of scope.

## Decision

1. **Vector-first**: embed the user query with the configured Ollama embedding model, score all chunk embeddings (cosine similarity), take top-k chunks.
2. **Graph augmentation**: derive candidate terms from the query and top chunks; match `graph_nodes.normalized_name` in DuckDB; include edges touching those nodes (single-hop listing in MVP; two-hop expansion can be layered without changing the UI contract).
3. **Fallback**: if the graph is empty or terms do not match, the prompt uses only chunk evidence (vector-only behavior).
4. **Synthesis**: Ollama chat completes a single turn with system instructions to stay within evidence.

## Consequences

- Retrieval is reproducible for the same index and models.
- DuckDB lives in the Python sidecar to avoid shipping `libduckdb` in the Rust binary; Rust delegates graph reads to JSON-RPC methods.
