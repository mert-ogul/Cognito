-- Document profile keyword index for resolve_scope (title, authors, page-1 head).
CREATE TABLE IF NOT EXISTS documents_profile (
  document_id TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
  profile TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
  profile,
  content='documents_profile',
  content_rowid='rowid',
  tokenize='porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS documents_profile_ai AFTER INSERT ON documents_profile BEGIN
  INSERT INTO documents_fts(rowid, profile) VALUES (new.rowid, new.profile);
END;

CREATE TRIGGER IF NOT EXISTS documents_profile_ad AFTER DELETE ON documents_profile BEGIN
  INSERT INTO documents_fts(documents_fts, rowid, profile) VALUES('delete', old.rowid, old.profile);
END;

CREATE TRIGGER IF NOT EXISTS documents_profile_au AFTER UPDATE ON documents_profile BEGIN
  INSERT INTO documents_fts(documents_fts, rowid, profile) VALUES('delete', old.rowid, old.profile);
  INSERT INTO documents_fts(rowid, profile) VALUES (new.rowid, new.profile);
END;
