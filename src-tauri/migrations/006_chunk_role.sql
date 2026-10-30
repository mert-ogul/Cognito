-- Body vs references role for chunks (graph skip + search weight).
ALTER TABLE chunks ADD COLUMN role TEXT NOT NULL DEFAULT 'body';
