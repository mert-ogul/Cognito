-- Replace DuckDB graph stage with SQLite entities + connections.

ALTER TABLE documents RENAME COLUMN graph_status TO entities_status;
ALTER TABLE documents RENAME COLUMN graph_error TO entities_error;

UPDATE documents SET entities_status = 'pending', entities_error = NULL
WHERE status = 'indexed';

INSERT INTO settings(key, value, updated_at)
SELECT 'extract_entities', value, datetime('now')
FROM settings WHERE key = 'graph_extraction_enabled';

DELETE FROM settings WHERE key = 'graph_extraction_enabled';

INSERT OR IGNORE INTO settings(key, value, updated_at)
VALUES ('extract_entities', 'true', datetime('now'));

INSERT OR IGNORE INTO settings(key, value, updated_at)
VALUES ('extra_entity_labels', '', datetime('now'));

CREATE TABLE IF NOT EXISTS entities (
  id TEXT PRIMARY KEY NOT NULL,
  label TEXT NOT NULL,
  name TEXT NOT NULL,
  normalized TEXT NOT NULL,
  doc_count INTEGER NOT NULL DEFAULT 0,
  mention_count INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_label_normalized
  ON entities(label, normalized);

CREATE TABLE IF NOT EXISTS entity_aliases (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  alias TEXT NOT NULL,
  UNIQUE(entity_id, alias)
);

CREATE TABLE IF NOT EXISTS mentions (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  chunk_id TEXT NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  start INTEGER NOT NULL,
  end INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mentions_entity ON mentions(entity_id);
CREATE INDEX IF NOT EXISTS idx_mentions_chunk ON mentions(chunk_id);
CREATE INDEX IF NOT EXISTS idx_mentions_document ON mentions(document_id);

CREATE TABLE IF NOT EXISTS document_links (
  source_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  target_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  weight REAL NOT NULL,
  evidence_chunk_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_document_links_source ON document_links(source_id);
CREATE INDEX IF NOT EXISTS idx_document_links_target ON document_links(target_id);

CREATE TABLE IF NOT EXISTS topics (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  document_ids TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS document_points (
  document_id TEXT PRIMARY KEY NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  x REAL NOT NULL,
  y REAL NOT NULL,
  topic_id TEXT
);
