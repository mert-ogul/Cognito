# ADR 005: Structured graph extraction

## Status

Accepted (supersedes LightRAG-in-sidecar)

## Context

Chat benefits from entity and relation hints drawn from chunked text. The first implementation called LightRAG's `extract_entities` in the Python sidecar. That prompt is thousands of tokens of instructions and delimiter examples. On small local models such as phi4-mini a single chunk took minutes, most replies had no relations, and IDs or addresses were often treated as entities. The sidecar also handles one RPC at a time, so a long extract blocked chat.

## Decision

Rust extracts each chunk with a short structured Ollama `/api/chat` call (`format` JSON schema, low `num_predict`, shared `num_ctx`). The system prompt asks for people, organizations, places, programs, concepts, events and documents as written, and short verb-phrase relations only between listed entities. Validated entities and relations are sent to the sidecar as `store_graph_chunk`, which normalizes names and upserts DuckDB in one transaction. The graph worker yields while chat is in flight so extraction never blocks answers. `COGNITO_MOCK_EXTRACT=1` returns a fixed two-node graph for tests.

## Consequences

Graph quality still depends on the chat model and chunk text, but extraction is fast enough for background rebuilds and produces usable relations. Per-document failures are recorded and do not wipe the rest of the library. Vector-only retrieval still works when extraction fails. Existing LightRAG graphs are requeued by migration 004.
