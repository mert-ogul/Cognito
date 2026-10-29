# ADR 006: DuckDB adjacency graph storage

## Status

Accepted

## Context

The product needs a local store for nodes and edges with simple SQL and bounded traversal. Shipping a separate graph server or linking DuckDB into the Rust binary was avoided.

## Decision

Adjacency lives in DuckDB tables `graph_nodes` and `graph_edges`. The Python sidecar is the only process that opens `graph.duckdb` for reads and writes. Rust passes the path over JSON-RPC for extraction, retrieval, and graph snapshots used by the UI.

## Consequences

Traversal uses explicit SQL with a hop limit in the retrieval pipeline. Backing up a library means copying SQLite, DuckDB, and the vault files together.
