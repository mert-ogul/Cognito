# ADR 007: DuckDB for adjacency graph storage

## Status

Accepted

## Context

The MVP requires a **local, embeddable** store for nodes and edges with simple SQL and good analytical performance. A separate graph server or large native dependency in the Rust binary was avoided.

## Decision

Store **adjacency** in **DuckDB** tables (`graph_nodes`, `graph_edges`) with indexes on source/target. The sidecar owns writes during ingestion; Rust ensures schema application; read-only visualization uses snapshots or filtered queries via RPC.

## Consequences

- No Cypher/GSQL in MVP; traversal is implemented with explicit SQL and bounded hops in the retrieval pipeline.
- File size stays on disk next to the vault; backup story is “copy SQLite + DuckDB + files.”

## Related

- [Revised MVP stack](./0002-revised-mvp-stack.md)
- [Security & privacy boundaries](./0004-security-privacy-boundaries.md)
