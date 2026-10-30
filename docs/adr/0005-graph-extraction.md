# ADR 005: Connections from extracted names

## Status

Accepted (supersedes ADR 006 and the earlier DuckDB / LightRAG graph)

## Context

Cognito used a background LLM pass to extract graph nodes and edges into DuckDB, then showed a Cytoscape graph pane. Extraction was slow, relation quality was uneven on small local models, and a second database made backup and purge harder. Document cards already surface people and organisations per file. What users need next is reliable names, places and dates that link documents, not a free-form knowledge graph.

## Decision

- Drop DuckDB (`graph.duckdb`), LightRAG-style relation extraction, and the Cytoscape graph UI.
- Store entities, aliases, mentions, document links, topics and document points in SQLite (migration 011). Rename `graph_status` / `graph_error` to `entities_status` / `entities_error`.
- Background worker order: extract entities (when `extract_entities` is on), then document cards, then `recompute_links` when idle.
- Setting `extract_entities` (default true) is labelled “Find names, places and dates”. Optional `extra_entity_labels` is a comma list of extra labels to look for. Label sets follow document card type (BASE plus type-specific labels). `rebuild_connections` requeues entity extraction.
- Chat retrieval stays hybrid vector + keyword over SQLite; it no longer takes a DuckDB path. Related-entity questions use `prepare_related_retrieval` then the same grounded answer path.
- The side pane keeps Passage. The Connections pane has Index, Map and Timeline views over entities, document points and dated cards / date entities.

## Consequences

One database holds documents, cards and connections. GLiNER runs only during extraction and is unloaded afterwards. Graph-style relation edges between arbitrary concepts are out of scope; shared names, mentions links and similarity replace them.
