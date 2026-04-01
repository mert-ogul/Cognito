# ADR 008: Deterministic hybrid retrieval instead of an adaptive classifier (MVP)

## Status

Accepted

## Context

Some systems route user questions through a **classifier** (“needs graph vs vector-only”) or adaptive tool selection. That adds model calls, non-determinism, and failure modes that are hard to explain in a privacy-sensitive desktop MVP.

## Decision

MVP retrieval is **fixed** and **deterministic**:

1. **Vector-first**: embed the query; rank chunks by similarity; take top-k.
2. **Graph augmentation**: derive terms from the query and retrieved chunks; match nodes in DuckDB; include a bounded subgraph in the prompt when present.
3. **Fallback**: if the graph is empty or fails, behavior is **explicit vector-only** (same pipeline without graph evidence).

No separate “router” model is required for MVP.

## Consequences

- Easier regression testing and benchmarking (same index + models → same retrieval shape).
- Future work: optional routing or tools behind feature flags, with new ADRs.

## Related

- [ADR 001: Deterministic hybrid retrieval](./0001-hybrid-retrieval.md)
