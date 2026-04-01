-- Per-document graph extraction stage (separate from vector index status)

ALTER TABLE documents ADD COLUMN graph_status TEXT NOT NULL DEFAULT 'none';
ALTER TABLE documents ADD COLUMN graph_error TEXT;

-- 'none' = not yet indexed or no graph pass; 'pending' = indexed, graph queued;
-- 'ok' = graph extracted; 'failed' = graph stage error; 'skipped' = no DuckDB / disabled

UPDATE documents SET graph_status = 'pending'
WHERE status = 'indexed' AND graph_status = 'none';
