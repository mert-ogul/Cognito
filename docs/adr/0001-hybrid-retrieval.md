# ADR 001: Deterministic hybrid retrieval

## Status

Accepted

## Context

Cognito answers questions from a local PDF, Word and Markdown library with numbered citations. An adaptive classifier that chooses between search modes would add model calls and make failures harder to explain.

## Decision

Retrieval is fixed. The query runs vector search and keyword search, and also matches names that appear in the question. Those ranked lists are fused by rank. A cross-encoder then reranks the fused candidates. The chat model answers from the kept passages, and each quote in the answer is checked against those passages before it is shown.

## Consequences

The same index and models produce the same retrieval shape, which keeps tests and debugging straightforward. Rerank or check failures surface as empty or refused answers rather than inventing unsupported text.
