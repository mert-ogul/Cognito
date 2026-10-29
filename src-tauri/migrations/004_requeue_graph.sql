-- Rebuild graphs with the structured Ollama extractor (replaces LightRAG results).
UPDATE documents SET graph_status = 'pending', graph_error = NULL
WHERE status = 'indexed';
