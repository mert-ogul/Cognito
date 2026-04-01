CREATE TABLE IF NOT EXISTS graph_nodes (
  node_id VARCHAR PRIMARY KEY,
  name VARCHAR NOT NULL,
  kind VARCHAR NOT NULL,
  normalized_name VARCHAR NOT NULL,
  source_count INTEGER NOT NULL,
  confidence_avg DOUBLE NOT NULL,
  metadata_json VARCHAR,
  created_at TIMESTAMP NOT NULL,
  updated_at TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS graph_edges (
  edge_id VARCHAR PRIMARY KEY,
  source_node_id VARCHAR NOT NULL,
  target_node_id VARCHAR NOT NULL,
  relation VARCHAR NOT NULL,
  document_id VARCHAR NOT NULL,
  chunk_id VARCHAR NOT NULL,
  confidence DOUBLE NOT NULL,
  created_at TIMESTAMP NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON graph_edges(source_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON graph_edges(target_node_id);
