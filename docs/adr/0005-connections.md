# ADR 005: Connections from extracted names

## Status

Accepted (supersedes the earlier DuckDB / LightRAG graph and the Map view)

## Context

Cognito used a background LLM pass to extract graph nodes and edges into a second database, then showed a free-form graph pane and later a Map of document points. Extraction was slow, relation quality was uneven on small local models, and a second database made backup and purge harder. Document cards already surface people and organisations per file. What users need is reliable names that link documents, not a free-form knowledge graph or a map of topics.

## Decision

Store entities, aliases, mentions and document links in SQLite. Rename the old graph status columns to `entities_status` / `entities_error`.

Background work extracts entities when “Find names, places and dates” is on, then builds document cards, then recomputes links when idle. Label sets start from a base list. When a document has a references section, paper labels are used as well. `rebuild_connections` requeues entity extraction.

The Connections pane keeps Index and Timeline. There is no Map. The document page lists related documents: shared names (weighted by rarity), mentions links, and similar content.

Chat retrieval stays hybrid over SQLite. Related-entity questions use the same grounded answer path.

## Consequences

One database holds documents, cards and connections. GLiNER runs only during extraction and is unloaded afterwards. Shared names, mentions and similarity replace graph-style relation edges.
