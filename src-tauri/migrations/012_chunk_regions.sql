-- Page regions on chunks (PDF bboxes) and scan status on documents.

ALTER TABLE chunks ADD COLUMN regions TEXT;
ALTER TABLE documents ADD COLUMN scan_status TEXT;
