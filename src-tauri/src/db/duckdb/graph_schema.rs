//! Migration helpers for legacy DuckDB files.

/// Best-effort upgrade for older schemas with nullable edge provenance columns.
pub fn migrate_legacy_edges(conn: &duckdb::Connection) {
    let _ = conn.execute(
        "DELETE FROM graph_edges WHERE document_id IS NULL OR chunk_id IS NULL",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE graph_edges ALTER COLUMN document_id SET NOT NULL",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE graph_edges ALTER COLUMN chunk_id SET NOT NULL",
        [],
    );
}
