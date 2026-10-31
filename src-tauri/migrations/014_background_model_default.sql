UPDATE settings
SET value = (
  SELECT c.value FROM settings AS c WHERE c.key = 'chat_model' LIMIT 1
),
updated_at = datetime('now')
WHERE key = 'background_model'
  AND value = 'qwen3.5:4b'
  AND EXISTS (SELECT 1 FROM settings AS c WHERE c.key = 'chat_model');
