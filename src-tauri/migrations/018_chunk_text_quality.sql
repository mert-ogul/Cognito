-- Per-chunk text quality for skipping OCR junk in search/embed.
ALTER TABLE chunks ADD COLUMN text_quality REAL;
