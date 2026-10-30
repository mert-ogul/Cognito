# ADR 006: DuckDB adjacency graph storage

## Status

Superseded by [ADR 005](0005-graph-extraction.md).

## Note

DuckDB `graph_nodes` / `graph_edges` are removed. Connections live in SQLite tables defined in migration 011 (`entities`, `mentions`, `document_links`, and related). See ADR 005 for the current decision.
