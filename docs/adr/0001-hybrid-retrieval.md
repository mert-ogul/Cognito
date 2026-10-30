# ADR 001: Deterministic hybrid retrieval

## Status

Accepted

## Context

Cognito answers questions from a local PDF and Markdown library with numbered citations. An adaptive classifier that chooses between graph and vector search would add model calls and make failures harder to explain.

## Decision

Retrieval is fixed. The query is embedded and chunks are ranked by vector similarity. Graph seeds come from edges attached to those chunks and from names that appear in the question. Expansion follows at most two hops in DuckDB. If the graph is empty or unavailable, the prompt uses the retrieved chunks alone. The chat model must cite evidence with numbered markers that match the citation list.

## Consequences

The same index and models produce the same retrieval shape, which keeps tests and debugging straightforward. Graph failures do not block chat when embeddings exist.
