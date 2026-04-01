//! DuckDB graph file: bootstrap schema for adjacency storage.

pub mod graph_queries;
pub mod graph_schema;

use crate::db::duckdb::graph_schema::migrate_legacy_edges;
use crate::errors::{CognitoError, CognitoResult};
use duckdb::Connection;
use std::path::Path;

const INIT_SQL: &str = include_str!("../../../migrations/duckdb_init.sql");

pub fn open_graph_store(path: &Path) -> CognitoResult<Connection> {
    let conn = Connection::open(path).map_err(|e| CognitoError::Database(e.to_string()))?;
    conn.execute_batch(INIT_SQL)
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    migrate_legacy_edges(&conn);
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn bootstrap_creates_nodes_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("g.duckdb");
        {
            let c = open_graph_store(&p).expect("open");
            let n: i64 = c
                .query_row("SELECT COUNT(*) FROM graph_nodes", [], |r| r.get(0))
                .expect("q");
            assert_eq!(n, 0);
        }
        drop(open_graph_store(&p).expect("reopen"));
        let _ = fs::remove_file(&p);
    }
}
