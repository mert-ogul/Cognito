//! SQLite vault access and migrations.

use crate::errors::{CognitoError, CognitoResult};
use crate::models::{DocumentRow, SettingsPayload};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const MIGRATION_001: &str = include_str!("../../../migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("../../../migrations/002_sqlite_vec_prep.sql");
const MIGRATION_003: &str = include_str!("../../../migrations/003_graph_stage.sql");

pub fn open_sqlite(path: &Path) -> CognitoResult<Connection> {
    let conn = Connection::open(path).map_err(|e| CognitoError::Database(e.to_string()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> CognitoResult<()> {
    let mut v: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    if v < 1 {
        conn.execute_batch(MIGRATION_001)
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        conn.execute("PRAGMA user_version = 1", [])
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        let _ = conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        );
        v = 1;
    }
    if v < 2 {
        conn.execute_batch(MIGRATION_002)
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        conn.execute("PRAGMA user_version = 2", [])
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        let _ = conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'))",
            [],
        );
        v = 2;
    }
    if v < 3 {
        conn.execute_batch(MIGRATION_003)
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        conn.execute("PRAGMA user_version = 3", [])
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        let _ = conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'))",
            [],
        );
    }
    Ok(())
}

pub fn upsert_setting(conn: &Connection, key: &str, value: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [key],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(row)
}

pub fn clear_setting(conn: &Connection, key: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn delete_all_documents(conn: &Connection) -> CognitoResult<()> {
    conn.execute("DELETE FROM documents", [])
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn maybe_set_index_embed_model(conn: &Connection, embed_model: &str) -> CognitoResult<()> {
    if get_setting(conn, "index_embed_model")?.is_none() {
        upsert_setting(conn, "index_embed_model", embed_model)?;
    }
    Ok(())
}

pub fn load_settings(conn: &Connection) -> CognitoResult<SettingsPayload> {
    let vault = get_setting(conn, "vault_path")?;
    let chat = get_setting(conn, "chat_model")?.unwrap_or_else(|| "phi4-mini".to_string());
    let embed = get_setting(conn, "embed_model")?.unwrap_or_else(|| "nomic-embed-text".to_string());
    let graph = get_setting(conn, "graph_extraction_enabled")?.unwrap_or_else(|| "true".to_string());
    Ok(SettingsPayload {
        vault_path: vault,
        chat_model: chat,
        embed_model: embed,
        graph_extraction_enabled: graph != "false",
    })
}

pub fn count_active_documents(conn: &Connection) -> CognitoResult<i64> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE status != 'removed'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(n)
}

pub fn list_documents(conn: &Connection) -> CognitoResult<Vec<DocumentRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, file_type, content_hash, page_count, status, error_message,
                    imported_at, indexed_at, modified_at, graph_status, graph_error
             FROM documents WHERE status != 'removed' ORDER BY imported_at DESC",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DocumentRow {
                id: r.get(0)?,
                path: r.get(1)?,
                title: r.get(2)?,
                file_type: r.get(3)?,
                content_hash: r.get(4)?,
                page_count: r.get::<_, Option<i64>>(5)?,
                status: r.get(6)?,
                error_message: r.get(7)?,
                imported_at: r.get(8)?,
                indexed_at: r.get(9)?,
                modified_at: r.get(10)?,
                graph_status: r.get(11)?,
                graph_error: r.get(12)?,
            })
        })
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn document_id_by_path(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT id FROM documents WHERE path = ?1 AND status != 'removed'",
            [path],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(row)
}

pub fn content_hash_by_path(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT content_hash FROM documents WHERE path = ?1 AND status = 'indexed'",
            [path],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(row)
}

/// Insert or update a document row as `failed` (clears hash so retry does not skip).
pub fn record_failed_document(
    conn: &Connection,
    path: &str,
    title: &str,
    file_type: &str,
    message: &str,
) -> CognitoResult<()> {
    let clip: String = message.chars().take(512).collect();
    let now = chrono::Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO documents (id, path, title, file_type, content_hash, page_count, status, error_message, imported_at, indexed_at, modified_at, graph_status, graph_error)
         VALUES (?1, ?2, ?3, ?4, '', NULL, 'failed', ?5, ?6, NULL, ?6, 'none', NULL)
         ON CONFLICT(path) DO UPDATE SET
           status = 'failed',
           error_message = excluded.error_message,
           content_hash = '',
           graph_status = 'none',
           graph_error = NULL,
           modified_at = excluded.modified_at",
        params![id, path, title, file_type, clip, now],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn document_id_including_removed(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row("SELECT id FROM documents WHERE path = ?1", [path], |r| r.get::<_, String>(0))
        .optional()
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(row)
}

/// Deletes chunk rows and marks the document removed. Returns document id for vec purge in sidecar.
pub fn mark_document_removed(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let Some(id) = document_id_including_removed(conn, path)? else {
        return Ok(None);
    };
    conn.execute(
        "DELETE FROM chunks WHERE document_id = ?1",
        [&id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'removed', modified_at = ?1, error_message = NULL WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(Some(id))
}

pub fn document_preview_text(conn: &Connection, doc_id: &str) -> CognitoResult<Option<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT text FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC LIMIT 3",
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut parts = Vec::new();
    let rows = stmt
        .query_map(params![doc_id], |r| r.get::<_, String>(0))
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    for r in rows {
        parts.push(r.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    if parts.is_empty() {
        return Ok(None);
    }
    let joined = parts.join("\n\n");
    let truncated = if joined.chars().count() > 4000 {
        joined.chars().take(4000).collect::<String>() + "…"
    } else {
        joined
    };
    Ok(Some(truncated))
}

pub fn failed_document_paths(conn: &Connection, limit: i64) -> CognitoResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT path FROM documents WHERE status = 'failed' ORDER BY imported_at LIMIT ?1")
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![limit], |r| r.get(0))
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn insert_import_job(conn: &Connection, id: &str, total: i64) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO import_jobs (id, status, total_files, completed_files, failed_files, started_at)
         VALUES (?1, 'running', ?2, 0, 0, ?3)",
        params![id, total, now],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn update_import_job_counts(
    conn: &Connection,
    id: &str,
    completed: i64,
    failed: i64,
) -> CognitoResult<()> {
    conn.execute(
        "UPDATE import_jobs SET completed_files = ?1, failed_files = ?2 WHERE id = ?3",
        params![completed, failed, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn finish_import_job(
    conn: &Connection,
    id: &str,
    status: &str,
    err: Option<&str>,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE import_jobs SET status = ?1, finished_at = ?2, error_message = ?3 WHERE id = ?4",
        params![status, now, err, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn list_chunk_ids_for_document(conn: &Connection, doc_id: &str) -> CognitoResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT id FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC")
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![doc_id], |r| r.get(0))
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CognitoError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn next_pending_graph_document(conn: &Connection) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT id FROM documents
             WHERE status = 'indexed' AND graph_status = 'pending'
             ORDER BY imported_at ASC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(row)
}

pub fn mark_document_graph_running(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET graph_status = 'running', graph_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn reset_running_graph_jobs(conn: &Connection) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET graph_status = 'pending', modified_at = ?1 WHERE graph_status = 'running'",
        params![now],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn document_graph_still_running(conn: &Connection, id: &str) -> CognitoResult<bool> {
    let row = conn
        .query_row(
            "SELECT 1 FROM documents WHERE id = ?1 AND status = 'indexed' AND graph_status = 'running'",
            [id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(row.is_some())
}

pub fn requeue_failed_graphs(conn: &Connection) -> CognitoResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn
        .execute(
            "UPDATE documents SET graph_status = 'pending', graph_error = NULL, modified_at = ?1
             WHERE status = 'indexed' AND graph_status = 'failed'",
            params![now],
        )
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(n)
}

pub fn mark_document_graph_pending(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET graph_status = 'pending', graph_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn delete_document_by_id(conn: &Connection, id: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM documents WHERE id = ?1", [id])
        .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_document_row(
    conn: &Connection,
    id: &str,
    path: &str,
    title: &str,
    file_type: &str,
    content_hash: &str,
    page_count: Option<i64>,
    status: &str,
    err: Option<&str>,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO documents (id, path, title, file_type, content_hash, page_count, status, error_message, imported_at, indexed_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?9)",
        params![id, path, title, file_type, content_hash, page_count, status, err, now],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn insert_chunk_row(
    conn: &Connection,
    id: &str,
    document_id: &str,
    ordinal: i64,
    page_start: Option<i64>,
    page_end: Option<i64>,
    text: &str,
    token_count: i64,
    content_hash: &str,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO chunks (id, document_id, ordinal, page_start, page_end, text, token_count, content_hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            document_id,
            ordinal,
            page_start,
            page_end,
            text,
            token_count,
            content_hash,
            now
        ],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn mark_document_indexed(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'indexed', indexed_at = ?1, modified_at = ?1, error_message = NULL,
         graph_status = 'pending', graph_error = NULL WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn mark_document_graph_ok(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET graph_status = 'ok', graph_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn mark_document_graph_failed(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET graph_status = 'failed', graph_error = ?1, modified_at = ?2 WHERE id = ?3",
        params![msg, now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

#[allow(dead_code)]
pub fn mark_document_graph_skipped(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET graph_status = 'skipped', graph_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

pub fn mark_document_failed(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'failed', error_message = ?1, modified_at = ?2, graph_status = 'none', graph_error = NULL WHERE id = ?3",
        params![msg, now, id],
    )
    .map_err(|e| CognitoError::Database(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn migration_applies_to_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        {
            let c = open_sqlite(&p).expect("open");
            let v: i32 = c
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("uv");
            assert_eq!(v, 3);
            let n: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'chunk_embeddings'",
                    [],
                    |r| r.get(0),
                )
                .expect("cnt");
            assert_eq!(n, 0);
        }
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn content_hash_by_path_only_indexed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c,
            "d1",
            "/a.md",
            "a",
            "markdown",
            "hash-failed",
            None,
            "failed",
            Some("boom"),
        )
        .expect("insert failed");
        assert!(content_hash_by_path(&c, "/a.md")
            .expect("q")
            .is_none());
        insert_document_row(
            &c,
            "d2",
            "/b.md",
            "b",
            "markdown",
            "hash-ok",
            None,
            "indexed",
            None,
        )
        .expect("insert indexed");
        assert_eq!(
            content_hash_by_path(&c, "/b.md").expect("q").as_deref(),
            Some("hash-ok")
        );
    }

    #[test]
    fn record_failed_document_inserts_and_updates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        record_failed_document(&c, "/x.md", "x", "markdown", "first").expect("insert");
        let (status, hash, err): (String, String, String) = c
            .query_row(
                "SELECT status, content_hash, error_message FROM documents WHERE path = ?1",
                ["/x.md"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(status, "failed");
        assert_eq!(hash, "");
        assert_eq!(err, "first");

        insert_document_row(
            &c,
            "old",
            "/y.md",
            "y",
            "markdown",
            "keep-me",
            None,
            "indexing",
            None,
        )
        .expect("indexing row");
        record_failed_document(&c, "/y.md", "y", "markdown", &"e".repeat(600)).expect("upsert");
        let (status, hash, err, gstat, gerr): (String, String, String, String, Option<String>) = c
            .query_row(
                "SELECT status, content_hash, error_message, graph_status, graph_error FROM documents WHERE path = ?1",
                ["/y.md"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("row");
        assert_eq!(status, "failed");
        assert_eq!(hash, "");
        assert_eq!(err.chars().count(), 512);
        assert_eq!(gstat, "none");
        assert!(gerr.is_none());
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE path = ?1",
                ["/y.md"],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(n, 1);
    }

    #[test]
    fn list_documents_excludes_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c,
            "keep",
            "/keep.md",
            "keep",
            "markdown",
            "h1",
            None,
            "indexed",
            None,
        )
        .expect("keep");
        insert_document_row(
            &c,
            "gone",
            "/gone.md",
            "gone",
            "markdown",
            "h2",
            None,
            "removed",
            None,
        )
        .expect("gone");
        let rows = list_documents(&c).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "keep");
    }
}
