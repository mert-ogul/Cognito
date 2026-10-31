use crate::errors::CognitoResult;
use crate::models::DocumentRow;
use rusqlite::{params, Connection, OptionalExtension};

use super::cards::{delete_document_card, rebuild_document_profile};
use super::entities::prune_entities;
use super::settings::parse_json_string_array;

pub fn delete_all_documents(conn: &Connection) -> CognitoResult<()> {
    conn.execute("DELETE FROM documents", [])?;
    prune_entities(conn)?;
    Ok(())
}

pub fn count_active_documents(conn: &Connection) -> CognitoResult<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM documents WHERE status != 'removed'",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

pub fn list_documents(conn: &Connection) -> CognitoResult<Vec<DocumentRow>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.path, d.title, d.file_type, d.content_hash, d.page_count, d.status,
                    d.error_message, d.imported_at, d.indexed_at, d.modified_at,
                    d.entities_status, d.entities_error, d.card_status,
                    c.type, c.date, c.people, c.organizations, d.scan_status
             FROM documents d
             LEFT JOIN document_cards c ON c.document_id = d.id
             WHERE d.status != 'removed'
             ORDER BY d.imported_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let people_raw: Option<String> = r.get(16)?;
        let orgs_raw: Option<String> = r.get(17)?;
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
            entities_status: r.get(11)?,
            entities_error: r.get(12)?,
            card_status: r.get(13)?,
            card_type: r.get(14)?,
            card_date: r.get(15)?,
            card_people: people_raw
                .as_deref()
                .map(parse_json_string_array)
                .unwrap_or_default(),
            card_organizations: orgs_raw
                .as_deref()
                .map(parse_json_string_array)
                .unwrap_or_default(),
            scan_status: r.get(18)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
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
        .optional()?;
    Ok(row)
}

pub fn content_hash_by_path(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT content_hash FROM documents WHERE path = ?1 AND status = 'indexed'",
            [path],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
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
        "INSERT INTO documents (id, path, title, file_type, content_hash, page_count, status, error_message, imported_at, indexed_at, modified_at, entities_status, entities_error)
         VALUES (?1, ?2, ?3, ?4, '', NULL, 'failed', ?5, ?6, NULL, ?6, 'none', NULL)
         ON CONFLICT(path) DO UPDATE SET
           status = 'failed',
           error_message = excluded.error_message,
           content_hash = '',
           entities_status = 'none',
           entities_error = NULL,
           modified_at = excluded.modified_at",
        params![id, path, title, file_type, clip, now],
    )?;
    Ok(())
}

pub fn document_id_including_removed(
    conn: &Connection,
    path: &str,
) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row("SELECT id FROM documents WHERE path = ?1", [path], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    Ok(row)
}

/// Deletes chunk rows and marks the document removed. Returns document id for vec purge in sidecar.
pub fn mark_document_removed(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let Some(id) = document_id_including_removed(conn, path)? else {
        return Ok(None);
    };
    conn.execute("DELETE FROM chunks WHERE document_id = ?1", [&id])?;
    delete_document_card(conn, &id)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'removed', modified_at = ?1, error_message = NULL WHERE id = ?2",
        params![now, id],
    )?;
    Ok(Some(id))
}

pub fn document_preview_text(conn: &Connection, doc_id: &str) -> CognitoResult<Option<String>> {
    let mut stmt = conn
        .prepare("SELECT text FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC LIMIT 3")?;
    let mut parts = Vec::new();
    let rows = stmt.query_map(params![doc_id], |r| r.get::<_, String>(0))?;
    for r in rows {
        parts.push(r?);
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

pub fn delete_document_by_id(conn: &Connection, id: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM documents WHERE id = ?1", [id])?;
    prune_entities(conn)?;
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
    scan_status: Option<&str>,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO documents (id, path, title, file_type, content_hash, page_count, status, error_message, imported_at, indexed_at, modified_at, scan_status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?9, ?10)",
        params![
            id,
            path,
            title,
            file_type,
            content_hash,
            page_count,
            status,
            err,
            now,
            scan_status
        ],
    )?;
    Ok(())
}

pub fn mark_document_indexed(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'indexed', indexed_at = ?1, modified_at = ?1, error_message = NULL,
         entities_status = 'pending', entities_error = NULL,
         card_status = 'pending', card_error = NULL WHERE id = ?2",
        params![now, id],
    )?;
    rebuild_document_profile(conn, id)?;
    Ok(())
}

pub fn mark_document_failed(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'failed', error_message = ?1, modified_at = ?2, entities_status = 'none', entities_error = NULL WHERE id = ?3",
        params![msg, now, id],
    )?;
    Ok(())
}
