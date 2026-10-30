-- Migrate stock defaults from the previous recommended chat/embed models.
UPDATE settings SET value = 'qwen3.5:9b', updated_at = datetime('now')
 WHERE key = 'chat_model' AND value IN ('phi4-mini', 'phi4-mini:latest');

UPDATE settings SET value = 'qwen3-embedding:0.6b', updated_at = datetime('now')
 WHERE key = 'embed_model' AND value IN ('nomic-embed-text', 'nomic-embed-text:latest');
