UPDATE settings SET value = 'false', updated_at = datetime('now')
WHERE key = 'graph_extraction_enabled' AND value = 'true';

CREATE TABLE IF NOT EXISTS document_cards (
  document_id TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  title TEXT NOT NULL,
  date TEXT,
  people TEXT NOT NULL DEFAULT '[]',
  organizations TEXT NOT NULL DEFAULT '[]',
  summary TEXT NOT NULL DEFAULT '[]',
  facts TEXT NOT NULL DEFAULT '[]',
  fields TEXT NOT NULL DEFAULT '[]',
  aliases TEXT NOT NULL DEFAULT '[]',
  questions TEXT NOT NULL DEFAULT '[]',
  model TEXT NOT NULL,
  created_at TEXT NOT NULL
);

ALTER TABLE documents ADD COLUMN card_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE documents ADD COLUMN card_error TEXT;

UPDATE documents SET card_status = 'pending', card_error = NULL
WHERE status = 'indexed';
