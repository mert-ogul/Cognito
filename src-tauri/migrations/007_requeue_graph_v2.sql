-- Rebuild graphs with idea-focused extraction, body-only chunks, and work nodes.
UPDATE documents SET graph_status = 'pending', graph_error = NULL
WHERE status = 'indexed';
