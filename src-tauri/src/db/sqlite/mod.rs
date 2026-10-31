//! SQLite vault access and migrations.

use crate::errors::CognitoResult;
use crate::models::{DocumentCardPayload, DocumentRow, SettingsPayload, SuggestedQuestion};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../../../migrations/001_initial.sql")),
    (2, include_str!("../../../migrations/002_sqlite_vec_prep.sql")),
    (3, include_str!("../../../migrations/003_graph_stage.sql")),
    (4, include_str!("../../../migrations/004_requeue_graph.sql")),
    (5, include_str!("../../../migrations/005_chunk_search.sql")),
    (6, include_str!("../../../migrations/006_chunk_role.sql")),
    (7, include_str!("../../../migrations/007_requeue_graph_v2.sql")),
    (8, include_str!("../../../migrations/008_default_models.sql")),
    (9, include_str!("../../../migrations/009_chunk_labels.sql")),
    (10, include_str!("../../../migrations/010_document_cards.sql")),
    (11, include_str!("../../../migrations/011_entities_connections.sql")),
    (12, include_str!("../../../migrations/012_chunk_regions.sql")),
    (13, include_str!("../../../migrations/013_documents_fts.sql")),
    (14, include_str!("../../../migrations/014_background_model_default.sql")),
    (15, include_str!("../../../migrations/015_drop_map_tables.sql")),
    (16, include_str!("../../../migrations/016_drop_schema_migrations.sql")),
];

/// Bump when embedding text format or retrieval index layout changes.
pub const INDEX_FORMAT_VERSION: i32 = 5;

pub fn open_sqlite(path: &Path) -> CognitoResult<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> CognitoResult<()> {
    let mut current: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for &(version, sql) in MIGRATIONS {
        if current >= version {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(&format!("PRAGMA user_version = {version}"), [])?;
        tx.commit()?;
        current = version;
    }
    Ok(())
}

pub fn upsert_setting(conn: &Connection, key: &str, value: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now],
    )?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [key],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(row)
}

pub fn clear_setting(conn: &Connection, key: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}

pub fn delete_all_documents(conn: &Connection) -> CognitoResult<()> {
    conn.execute("DELETE FROM documents", [])?;
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
    let chat = get_setting(conn, "chat_model")?.unwrap_or_else(|| "qwen3.5:9b".to_string());
    let embed =
        get_setting(conn, "embed_model")?.unwrap_or_else(|| "qwen3-embedding:0.6b".to_string());
    let background =
        get_setting(conn, "background_model")?.unwrap_or_else(|| chat.clone());
    let summarise =
        get_setting(conn, "summarise_documents")?.unwrap_or_else(|| "true".to_string());
    let extract =
        get_setting(conn, "extract_entities")?.unwrap_or_else(|| "true".to_string());
    let extra_labels =
        get_setting(conn, "extra_entity_labels")?.unwrap_or_else(|| String::new());
    Ok(SettingsPayload {
        vault_path: vault,
        chat_model: chat,
        embed_model: embed,
        background_model: background,
        summarise_documents: summarise != "false",
        extract_entities: extract != "false",
        extra_entity_labels: extra_labels,
    })
}

fn parse_json_string_array(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s),
            Value::Object(map) => map
                .get("name")
                .and_then(|n| n.as_str())
                .map(str::to_string),
            _ => None,
        })
        .collect()
}

fn parse_json_value_array(raw: &str) -> Vec<Value> {
    serde_json::from_str(raw).unwrap_or_else(|_| Vec::new())
}

pub fn count_active_documents(conn: &Connection) -> CognitoResult<i64> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE status != 'removed'",
            [],
            |r| r.get(0),
        )?;
    Ok(n)
}

pub fn list_documents(conn: &Connection) -> CognitoResult<Vec<DocumentRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT d.id, d.path, d.title, d.file_type, d.content_hash, d.page_count, d.status,
                    d.error_message, d.imported_at, d.indexed_at, d.modified_at,
                    d.entities_status, d.entities_error, d.card_status,
                    c.type, c.date, c.people, c.organizations, d.scan_status
             FROM documents d
             LEFT JOIN document_cards c ON c.document_id = d.id
             WHERE d.status != 'removed'
             ORDER BY d.imported_at DESC",
        )?;
    let rows = stmt
        .query_map([], |r| {
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

pub fn document_id_including_removed(conn: &Connection, path: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row("SELECT id FROM documents WHERE path = ?1", [path], |r| r.get::<_, String>(0))
        .optional()?;
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
    )?;
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
        .prepare(
            "SELECT text FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC LIMIT 3",
        )?;
    let mut parts = Vec::new();
    let rows = stmt
        .query_map(params![doc_id], |r| r.get::<_, String>(0))?;
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

#[derive(Debug, Clone)]
pub struct ChunkWindow {
    pub text: String,
    pub highlight: String,
    pub page_start: Option<i64>,
    pub page_end: Option<i64>,
    pub regions: Option<String>,
}

pub fn chunk_window(
    conn: &Connection,
    document_id: &str,
    chunk_id: &str,
) -> CognitoResult<Option<ChunkWindow>> {
    let row: Option<(i64, String, Option<i64>, Option<i64>, Option<String>)> = conn
        .query_row(
            "SELECT ordinal, text, page_start, page_end, regions FROM chunks
             WHERE id = ?1 AND document_id = ?2",
            params![chunk_id, document_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((ordinal, cited, page_start, page_end, regions)) = row else {
        return Ok(None);
    };
    let prev: Option<String> = conn
        .query_row(
            "SELECT text FROM chunks WHERE document_id = ?1 AND ordinal = ?2",
            params![document_id, ordinal - 1],
            |r| r.get(0),
        )
        .optional()?;
    let next: Option<String> = conn
        .query_row(
            "SELECT text FROM chunks WHERE document_id = ?1 AND ordinal = ?2",
            params![document_id, ordinal + 1],
            |r| r.get(0),
        )
        .optional()?;
    let mut parts = Vec::new();
    if let Some(p) = prev {
        parts.push(p);
    }
    parts.push(cited.clone());
    if let Some(n) = next {
        parts.push(n);
    }
    Ok(Some(ChunkWindow {
        text: parts.join("\n\n"),
        highlight: cited,
        page_start,
        page_end,
        regions,
    }))
}

pub fn document_path_by_id(conn: &Connection, id: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT path FROM documents WHERE id = ?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(row)
}

pub fn document_path_and_hash_by_id(
    conn: &Connection,
    id: &str,
) -> CognitoResult<Option<(String, String)>> {
    let row = conn
        .query_row(
            "SELECT path, content_hash FROM documents WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(row)
}

pub fn document_scan_status(conn: &Connection, id: &str) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT scan_status FROM documents WHERE id = ?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(row.flatten())
}

pub fn failed_document_paths(conn: &Connection, limit: i64) -> CognitoResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT path FROM documents WHERE status = 'failed' ORDER BY imported_at LIMIT ?1")?;
    let rows = stmt
        .query_map(params![limit], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn insert_import_job(conn: &Connection, id: &str, total: i64) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO import_jobs (id, status, total_files, completed_files, failed_files, started_at)
         VALUES (?1, 'running', ?2, 0, 0, ?3)",
        params![id, total, now],
    )?;
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
    )?;
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
    )?;
    Ok(())
}

pub fn list_chunk_ids_for_document(conn: &Connection, doc_id: &str) -> CognitoResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT id FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC")?;
    let rows = stmt
        .query_map(params![doc_id], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn next_pending_entities_document(conn: &Connection) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT id FROM documents
             WHERE status = 'indexed' AND entities_status = 'pending'
             ORDER BY imported_at ASC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(row)
}

pub fn mark_document_entities_running(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET entities_status = 'running', entities_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn reset_running_entities_jobs(conn: &Connection) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET entities_status = 'pending', modified_at = ?1 WHERE entities_status = 'running'",
        params![now],
    )?;
    Ok(())
}

pub fn document_entities_still_running(conn: &Connection, id: &str) -> CognitoResult<bool> {
    let row = conn
        .query_row(
            "SELECT 1 FROM documents WHERE id = ?1 AND status = 'indexed' AND entities_status = 'running'",
            [id],
            |_| Ok(()),
        )
        .optional()?;
    Ok(row.is_some())
}

pub fn requeue_failed_entities(conn: &Connection) -> CognitoResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn
        .execute(
            "UPDATE documents SET entities_status = 'pending', entities_error = NULL, modified_at = ?1
             WHERE status = 'indexed' AND entities_status = 'failed'",
            params![now],
        )?;
    Ok(n)
}

pub fn requeue_all_entities(conn: &Connection) -> CognitoResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn
        .execute(
            "UPDATE documents SET entities_status = 'pending', entities_error = NULL, modified_at = ?1
             WHERE status = 'indexed'",
            params![now],
        )?;
    Ok(n)
}

pub fn mark_document_entities_pending(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET entities_status = 'pending', entities_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn mark_document_entities_ok(
    conn: &Connection,
    id: &str,
    note: Option<&str>,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET entities_status = 'ok', entities_error = ?1, modified_at = ?2 WHERE id = ?3",
        params![note, now, id],
    )?;
    Ok(())
}

pub fn mark_document_entities_failed(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET entities_status = 'failed', entities_error = ?1, modified_at = ?2 WHERE id = ?3",
        params![msg, now, id],
    )?;
    Ok(())
}

pub fn clear_document_entity_rows(conn: &Connection, document_id: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM mentions WHERE document_id = ?1", [document_id])?;
    conn.execute(
        "DELETE FROM document_links WHERE source_id = ?1 OR target_id = ?1",
        [document_id],
    )?;
    Ok(())
}

pub fn list_entities_index(conn: &Connection) -> CognitoResult<Vec<Value>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, normalized, label, mention_count, doc_count
             FROM entities ORDER BY mention_count DESC, name ASC LIMIT 500",
        )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "normalized": r.get::<_, String>(2)?,
                "label": r.get::<_, String>(3)?,
                "mentionCount": r.get::<_, i64>(4)?,
                "docCount": r.get::<_, i64>(5)?,
            }))
        })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn entity_detail(conn: &Connection, entity_id: &str) -> CognitoResult<Option<Value>> {
    let base = conn
        .query_row(
            "SELECT id, name, normalized, label, mention_count, doc_count FROM entities WHERE id = ?1",
            [entity_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((id, name, normalized, label, mention_count, doc_count)) = base else {
        return Ok(None);
    };

    let mut alias_stmt = conn
        .prepare("SELECT alias FROM entity_aliases WHERE entity_id = ?1 ORDER BY alias")?;
    let aliases: Vec<String> = alias_stmt
        .query_map([entity_id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut doc_stmt = conn
        .prepare(
            "SELECT DISTINCT d.id, d.title, d.path
             FROM mentions m
             JOIN documents d ON d.id = m.document_id
             WHERE m.entity_id = ?1
             ORDER BY d.title COLLATE NOCASE, d.id",
        )?;
    let doc_rows: Vec<(String, String, String)> = doc_stmt
        .query_map([entity_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut passage_stmt = conn
        .prepare(
            "SELECT m.chunk_id, c.text, c.page_start, c.page_end, m.start, m.end
             FROM mentions m
             JOIN chunks c ON c.id = m.chunk_id
             WHERE m.entity_id = ?1 AND m.document_id = ?2
             ORDER BY c.ordinal, m.start
             LIMIT 40",
        )?;

    let mut documents = Vec::new();
    for (document_id, title, path) in doc_rows {
        let passages: Vec<Value> = passage_stmt
            .query_map(params![entity_id, document_id], |r| {
                let chunk_id: String = r.get(0)?;
                let text: String = r.get(1)?;
                let page_start: Option<i64> = r.get(2)?;
                let page_end: Option<i64> = r.get(3)?;
                let start: i64 = r.get(4)?;
                let end: i64 = r.get(5)?;
                Ok(serde_json::json!({
                    "chunkId": chunk_id,
                    "snippet": mention_snippet(&text, start, end),
                    "pageStart": page_start,
                    "pageEnd": page_end,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        documents.push(serde_json::json!({
            "documentId": document_id,
            "title": title,
            "path": path,
            "passages": passages,
        }));
    }

    let mut co_stmt = conn
        .prepare(
            "SELECT e.id, e.name, e.label, COUNT(*) AS cnt
             FROM mentions m1
             JOIN mentions m2
               ON m1.chunk_id = m2.chunk_id AND m2.entity_id != m1.entity_id
             JOIN entities e ON e.id = m2.entity_id
             WHERE m1.entity_id = ?1
             GROUP BY e.id
             ORDER BY cnt DESC, e.name COLLATE NOCASE ASC
             LIMIT 12",
        )?;
    let co_mentions: Vec<Value> = co_stmt
        .query_map([entity_id], |r| {
            Ok(serde_json::json!({
                "entityId": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "label": r.get::<_, String>(2)?,
                "count": r.get::<_, i64>(3)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(serde_json::json!({
        "id": id,
        "name": name,
        "label": label,
        "normalized": normalized,
        "docCount": doc_count,
        "mentionCount": mention_count,
        "aliases": aliases,
        "documents": documents,
        "coMentions": co_mentions,
    })))
}

fn mention_snippet(text: &str, start: i64, end: i64) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let start = (start.max(0) as usize).min(chars.len());
    let end = (end.max(0) as usize).min(chars.len()).max(start);
    let window = 120usize;
    let from = start.saturating_sub(window);
    let to = (end + window).min(chars.len());
    let mut snippet: String = chars[from..to].iter().collect();
    if from > 0 {
        snippet.insert_str(0, "…");
    }
    if to < chars.len() {
        snippet.push('…');
    }
    snippet
}

fn related_link_kind_label(kind: &str) -> Option<&'static str> {
    match kind {
        "mentions" => Some("mentions this document"),
        "similar" => Some("similar content"),
        _ => None,
    }
}

fn entity_label_rank(label: &str) -> i32 {
    match label.trim().to_ascii_lowercase().as_str() {
        "person" => 0,
        "organization" => 1,
        _ => 2,
    }
}

/// Up to 8 related documents: shared entities (rarity-weighted) plus document_links.
pub fn list_related_documents(conn: &Connection, document_id: &str) -> CognitoResult<Vec<Value>> {
    use std::collections::HashMap;

    #[derive(Default)]
    struct Acc {
        title: String,
        score: f64,
        shared: Vec<(String, String, i64)>,
        mentions: bool,
        similar: bool,
    }

    let mut by_id: HashMap<String, Acc> = HashMap::new();

    let mut share_stmt = conn
        .prepare(
            "SELECT d.id, IFNULL(d.title, ''), e.name, e.label, e.doc_count
             FROM mentions m1
             JOIN mentions m2
               ON m1.entity_id = m2.entity_id AND m2.document_id != m1.document_id
             JOIN entities e ON e.id = m1.entity_id
             JOIN documents d ON d.id = m2.document_id
             WHERE m1.document_id = ?1
               AND lower(e.label) != 'date'
               AND d.status = 'indexed'
             GROUP BY d.id, e.id",
        )?;
    let share_rows = share_stmt
        .query_map([document_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
    for row in share_rows {
        let (oid, title, name, label, doc_count) =
            row?;
        let rarity = if doc_count > 0 {
            1.0 / (doc_count as f64)
        } else {
            0.0
        };
        let entry = by_id.entry(oid).or_default();
        if entry.title.is_empty() {
            entry.title = title;
        }
        entry.score += rarity;
        entry.shared.push((name, label, doc_count));
    }

    let mut mention_stmt = conn
        .prepare(
            "SELECT l.source_id, IFNULL(d.title, '')
             FROM document_links l
             JOIN documents d ON d.id = l.source_id
             WHERE l.kind = 'mentions' AND l.target_id = ?1 AND d.status = 'indexed'",
        )?;
    let mention_rows = mention_stmt
        .query_map([document_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
    for row in mention_rows {
        let (oid, title) = row?;
        let entry = by_id.entry(oid).or_default();
        if entry.title.is_empty() {
            entry.title = title;
        }
        entry.mentions = true;
        entry.score += 0.01;
    }

    let mut similar_stmt = conn
        .prepare(
            "SELECT
                CASE WHEN source_id = ?1 THEN target_id ELSE source_id END AS other_id,
                IFNULL(d.title, '')
             FROM document_links l
             JOIN documents d
               ON d.id = CASE WHEN l.source_id = ?1 THEN l.target_id ELSE l.source_id END
             WHERE l.kind = 'similar'
               AND (l.source_id = ?1 OR l.target_id = ?1)
               AND d.status = 'indexed'",
        )?;
    let similar_rows = similar_stmt
        .query_map([document_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
    for row in similar_rows {
        let (oid, title) = row?;
        if oid == document_id {
            continue;
        }
        let entry = by_id.entry(oid).or_default();
        if entry.title.is_empty() {
            entry.title = title;
        }
        entry.similar = true;
        entry.score += 0.001;
    }

    let mut ranked: Vec<(String, Acc)> = by_id.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.score
            .partial_cmp(&a.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.title.cmp(&b.1.title))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut out = Vec::new();
    for (id, mut acc) in ranked.into_iter().take(8) {
        acc.shared.sort_by(|a, b| {
            entity_label_rank(&a.1)
                .cmp(&entity_label_rank(&b.1))
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.0.cmp(&b.0))
        });
        acc.shared.dedup_by(|a, b| a.0 == b.0);
        let shared_names: Vec<String> = acc
            .shared
            .into_iter()
            .take(5)
            .map(|(name, _, _)| name)
            .collect();

        let (kind, kind_label) = if acc.mentions {
            ("mentions", related_link_kind_label("mentions").unwrap())
        } else if !shared_names.is_empty() {
            ("shares_names", "shares names")
        } else if acc.similar {
            ("similar", related_link_kind_label("similar").unwrap())
        } else {
            ("shares_names", "shares names")
        };

        out.push(serde_json::json!({
            "id": id,
            "title": acc.title,
            "kind": kind,
            "kindLabel": kind_label,
            "sharedEntities": shared_names,
        }));
    }
    Ok(out)
}

pub fn connections_timeline(conn: &Connection) -> CognitoResult<Value> {
    let mut items = Vec::new();

    let mut card_stmt = conn
        .prepare(
            "SELECT c.document_id, c.title, c.date
             FROM document_cards c
             WHERE c.date IS NOT NULL AND TRIM(c.date) != ''
             ORDER BY c.date ASC, c.title COLLATE NOCASE ASC
             LIMIT 500",
        )?;
    let card_rows = card_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
    for row in card_rows {
        let (document_id, title, date) =
            row?;
        items.push(serde_json::json!({
            "kind": "document",
            "id": document_id,
            "title": title,
            "date": date,
            "documentId": document_id,
        }));
    }

    let mut date_stmt = conn
        .prepare(
            "SELECT id, name, normalized
             FROM entities
             WHERE lower(label) = 'date'
               AND normalized IS NOT NULL AND TRIM(normalized) != ''
             ORDER BY normalized ASC, name COLLATE NOCASE ASC
             LIMIT 500",
        )?;
    let date_rows = date_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
    for row in date_rows {
        let (entity_id, name, date) =
            row?;
        items.push(serde_json::json!({
            "kind": "entity",
            "id": entity_id,
            "name": name,
            "date": date,
            "entityId": entity_id,
        }));
    }

    Ok(serde_json::json!({ "items": items }))
}

pub fn entity_names(conn: &Connection, a: &str, b: &str) -> CognitoResult<(String, String)> {
    let name_of = |id: &str| -> CognitoResult<String> {
        Ok(conn.query_row("SELECT name FROM entities WHERE id = ?1", [id], |r| r.get(0))?)
    };
    Ok((name_of(a)?, name_of(b)?))
}

pub fn pending_entities_or_cards_remain(conn: &Connection) -> CognitoResult<bool> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE status = 'indexed' AND (
                entities_status IN ('pending', 'running')
                OR card_status IN ('pending', 'running')
             )",
            [],
            |r| r.get(0),
        )?;
    Ok(n > 0)
}

pub fn delete_document_by_id(conn: &Connection, id: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM documents WHERE id = ?1", [id])?;
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
    role: &str,
    labels: &str,
    regions: Option<&str>,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let role = if role == "references" {
        "references"
    } else {
        "body"
    };
    conn.execute(
        "INSERT INTO chunks (id, document_id, ordinal, page_start, page_end, text, token_count, content_hash, created_at, role, labels, regions)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            document_id,
            ordinal,
            page_start,
            page_end,
            text,
            token_count,
            content_hash,
            now,
            role,
            labels,
            regions
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

fn page1_head_text(conn: &Connection, document_id: &str) -> CognitoResult<String> {
    let mut stmt = conn
        .prepare(
            "SELECT text FROM chunks
             WHERE document_id = ?1
               AND (page_start IS NULL OR page_start <= 1)
             ORDER BY ordinal",
        )?;
    let mut out = String::new();
    let rows = stmt
        .query_map([document_id], |r| r.get::<_, String>(0))?;
    for row in rows {
        let t = row?;
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&t);
        if out.chars().count() >= 1500 {
            break;
        }
    }
    Ok(out.chars().take(1500).collect())
}

fn json_name_list(raw: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return vec![];
    };
    let Some(arr) = v.as_array() else {
        return vec![];
    };
    let mut out = Vec::new();
    for item in arr {
        if let Some(s) = item.as_str() {
            if !s.trim().is_empty() {
                out.push(s.to_string());
            }
            continue;
        }
        if let Some(obj) = item.as_object() {
            if let Some(name) = obj.get("name").and_then(|x| x.as_str()) {
                if !name.trim().is_empty() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// Rebuild documents_profile + documents_fts for one document.
pub fn rebuild_document_profile(conn: &Connection, document_id: &str) -> CognitoResult<()> {
    let row = conn
        .query_row(
            "SELECT title, path FROM documents WHERE id = ?1",
            [document_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((title, path)) = row else {
        return Ok(());
    };
    let (people_raw, orgs_raw, aliases_raw) = conn
        .query_row(
            "SELECT people, organizations, aliases FROM document_cards WHERE document_id = ?1",
            [document_id],
            |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_else(|_| "[]".into()),
                    r.get::<_, String>(1).unwrap_or_else(|_| "[]".into()),
                    r.get::<_, String>(2).unwrap_or_else(|_| "[]".into()),
                ))
            },
        )
        .optional()?
        .unwrap_or_else(|| ("[]".into(), "[]".into(), "[]".into()));

    let stem = Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .replace('_', " ")
        .replace('-', " ");
    let mut parts: Vec<String> = Vec::new();
    if !title.trim().is_empty() {
        parts.push(title.trim().to_string());
    }
    if !stem.trim().is_empty() {
        parts.push(stem);
    }
    for name in json_name_list(&people_raw) {
        parts.push(name);
    }
    for name in json_name_list(&orgs_raw) {
        parts.push(name);
    }
    for name in json_name_list(&aliases_raw) {
        parts.push(name);
    }
    let head = page1_head_text(conn, document_id)?;
    if !head.trim().is_empty() {
        parts.push(head);
    }
    let profile = parts.join("\n");
    conn.execute(
        "INSERT INTO documents_profile(document_id, profile) VALUES (?1, ?2)
         ON CONFLICT(document_id) DO UPDATE SET profile = excluded.profile",
        params![document_id, profile],
    )?;
    Ok(())
}

pub fn upsert_document_card(
    conn: &Connection,
    document_id: &str,
    card_type: &str,
    title: &str,
    date: Option<&str>,
    people: &str,
    organizations: &str,
    summary: &str,
    facts: &str,
    fields: &str,
    aliases: &str,
    questions: &str,
    model: &str,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO document_cards (
            document_id, type, title, date, people, organizations, summary, facts, fields,
            aliases, questions, model, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(document_id) DO UPDATE SET
            type = excluded.type,
            title = excluded.title,
            date = excluded.date,
            people = excluded.people,
            organizations = excluded.organizations,
            summary = excluded.summary,
            facts = excluded.facts,
            fields = excluded.fields,
            aliases = excluded.aliases,
            questions = excluded.questions,
            model = excluded.model,
            created_at = excluded.created_at",
        params![
            document_id,
            card_type,
            title,
            date,
            people,
            organizations,
            summary,
            facts,
            fields,
            aliases,
            questions,
            model,
            now
        ],
    )?;
    rebuild_document_profile(conn, document_id)?;
    Ok(())
}

pub fn get_document_card(
    conn: &Connection,
    document_id: &str,
) -> CognitoResult<Option<DocumentCardPayload>> {
    let row = conn
        .query_row(
            "SELECT document_id, type, title, date, people, organizations, summary, facts, fields,
                    aliases, questions, model, created_at
             FROM document_cards WHERE document_id = ?1",
            [document_id],
            |r| {
                let people: String = r.get(4)?;
                let organizations: String = r.get(5)?;
                let summary: String = r.get(6)?;
                let facts: String = r.get(7)?;
                let fields: String = r.get(8)?;
                let aliases: String = r.get(9)?;
                let questions: String = r.get(10)?;
                Ok(DocumentCardPayload {
                    document_id: r.get(0)?,
                    card_type: r.get(1)?,
                    title: r.get(2)?,
                    date: r.get(3)?,
                    people: parse_json_value_array(&people),
                    organizations: parse_json_string_array(&organizations),
                    summary: parse_json_value_array(&summary),
                    facts: parse_json_value_array(&facts),
                    fields: parse_json_value_array(&fields),
                    aliases: parse_json_string_array(&aliases),
                    questions: parse_json_string_array(&questions),
                    model: r.get(11)?,
                    created_at: r.get(12)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn delete_document_card(conn: &Connection, document_id: &str) -> CognitoResult<()> {
    conn.execute(
        "DELETE FROM document_cards WHERE document_id = ?1",
        [document_id],
    )?;
    Ok(())
}

pub fn next_pending_card_document(conn: &Connection) -> CognitoResult<Option<String>> {
    let row = conn
        .query_row(
            "SELECT id FROM documents
             WHERE status = 'indexed' AND card_status = 'pending'
             ORDER BY indexed_at ASC, imported_at ASC
             LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(row)
}

pub fn mark_document_card_pending(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET card_status = 'pending', card_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn mark_document_card_running(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET card_status = 'running', card_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn mark_document_card_ok(conn: &Connection, id: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET card_status = 'ok', card_error = NULL, modified_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn mark_document_card_failed(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET card_status = 'failed', card_error = ?1, modified_at = ?2 WHERE id = ?3",
        params![msg, now, id],
    )?;
    Ok(())
}

pub fn reset_running_card_jobs(conn: &Connection) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET card_status = 'pending', modified_at = ?1 WHERE card_status = 'running'",
        params![now],
    )?;
    Ok(())
}

pub fn document_card_still_running(conn: &Connection, id: &str) -> CognitoResult<bool> {
    let row = conn
        .query_row(
            "SELECT 1 FROM documents WHERE id = ?1 AND status = 'indexed' AND card_status = 'running'",
            [id],
            |_| Ok(()),
        )
        .optional()?;
    Ok(row.is_some())
}

pub fn update_document_title(conn: &Connection, id: &str, title: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET title = ?1, modified_at = ?2 WHERE id = ?3",
        params![title, now, id],
    )?;
    Ok(())
}

pub fn document_meta_for_card(
    conn: &Connection,
    id: &str,
) -> CognitoResult<Option<(String, String, Option<i64>)>> {
    let row = conn
        .query_row(
            "SELECT title, path, page_count FROM documents WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row)
}

pub fn list_chunk_texts_ordered(conn: &Connection, doc_id: &str) -> CognitoResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT text FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC")?;
    let rows = stmt
        .query_map(params![doc_id], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_suggested_questions(conn: &Connection) -> CognitoResult<Vec<SuggestedQuestion>> {
    let mut stmt = conn
        .prepare(
            "SELECT c.document_id, c.questions, c.title, c.people, c.organizations, d.path
             FROM document_cards c
             INNER JOIN documents d ON d.id = c.document_id
             WHERE d.status = 'indexed' AND d.card_status = 'ok'
             ORDER BY d.indexed_at DESC",
        )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;

    let mut from_cards = Vec::new();
    // entity_key (lower) -> (display name, docs: (doc_id, file_name) in recent-first order)
    let mut entity_docs: std::collections::HashMap<String, (String, Vec<(String, String)>)> =
        std::collections::HashMap::new();

    for row in rows {
        let (doc_id, questions_raw, title, people_raw, orgs_raw, path) =
            row?;
        for q in parse_json_string_array(&questions_raw) {
            if q.trim().is_empty() {
                continue;
            }
            from_cards.push(SuggestedQuestion {
                question: q,
                scope_ids: vec![doc_id.clone()],
                label: Some(title.clone()),
            });
        }

        let file_name = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&path)
            .to_string();
        let mut names = parse_json_string_array(&people_raw);
        names.extend(parse_json_string_array(&orgs_raw));
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            let entry = entity_docs
                .entry(key)
                .or_insert_with(|| (trimmed.to_string(), Vec::new()));
            if !entry.1.iter().any(|(id, _)| id == &doc_id) {
                entry.1.push((doc_id.clone(), file_name.clone()));
            }
        }
    }

    let mut cross_doc = Vec::new();
    for (_key, (entity, docs)) in entity_docs {
        if docs.len() < 2 {
            continue;
        }
        let (id_a, file_a) = &docs[0];
        let (id_b, file_b) = &docs[1];
        cross_doc.push(SuggestedQuestion {
            question: format!(
                "{file_a} and {file_b} both mention {entity}. What do they say about it?"
            ),
            scope_ids: vec![id_a.clone(), id_b.clone()],
            label: None,
        });
    }

    let mut out = Vec::new();
    for q in from_cards {
        if out.len() >= 5 {
            break;
        }
        out.push(q);
    }
    for q in cross_doc {
        if out.len() >= 5 {
            break;
        }
        out.push(q);
    }
    Ok(out)
}

pub fn mark_document_failed(conn: &Connection, id: &str, msg: &str) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE documents SET status = 'failed', error_message = ?1, modified_at = ?2, entities_status = 'none', entities_error = NULL WHERE id = ?3",
        params![msg, now, id],
    )?;
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
            assert_eq!(v, 16);
            assert_eq!(INDEX_FORMAT_VERSION, 5);
            let has_topics: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='topics'",
                    [],
                    |r| r.get(0),
                )
                .expect("topics");
            assert_eq!(has_topics, 0);
            let has_points: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='document_points'",
                    [],
                    |r| r.get(0),
                )
                .expect("document_points");
            assert_eq!(has_points, 0);
            let has_schema_migrations: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
                    [],
                    |r| r.get(0),
                )
                .expect("schema_migrations");
            assert_eq!(has_schema_migrations, 0);
            let has_profile: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='documents_profile'",
                    [],
                    |r| r.get(0),
                )
                .expect("documents_profile");
            assert_eq!(has_profile, 1);
            let has_regions: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name = 'regions'",
                    [],
                    |r| r.get(0),
                )
                .expect("regions col");
            assert_eq!(has_regions, 1);
            let has_scan: i32 = c
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name = 'scan_status'",
                    [],
                    |r| r.get(0),
                )
                .expect("scan_status col");
            assert_eq!(has_scan, 1);
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
            None,
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
            None,
        )
        .expect("indexing row");
        record_failed_document(&c, "/y.md", "y", "markdown", &"e".repeat(600)).expect("upsert");
        let (status, hash, err, estat, eerr): (String, String, String, String, Option<String>) = c
            .query_row(
                "SELECT status, content_hash, error_message, entities_status, entities_error FROM documents WHERE path = ?1",
                ["/y.md"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("row");
        assert_eq!(status, "failed");
        assert_eq!(hash, "");
        assert_eq!(err.chars().count(), 512);
        assert_eq!(estat, "none");
        assert!(eerr.is_none());
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
            None,
        )
        .expect("gone");
        let rows = list_documents(&c).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "keep");
    }

    #[test]
    fn chunks_fts_stays_in_sync_on_insert_and_delete() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c,
            "d1",
            "/a.md",
            "a",
            "markdown",
            "h",
            None,
            "indexed",
            None,
            None,
        )
        .expect("doc");
        insert_chunk_row(
            &c,
            "c1",
            "d1",
            0,
            None,
            None,
            "renormalization trick GCN",
            4,
            "h1",
            "body",
            "[]",
            None,
        )
        .expect("c1");
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'GCN'",
                [],
                |r| r.get(0),
            )
            .expect("fts match");
        assert_eq!(n, 1);
        c.execute("DELETE FROM chunks WHERE id = ?1", ["c1"])
            .expect("delete");
        let n2: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'GCN'",
                [],
                |r| r.get(0),
            )
            .expect("fts after delete");
        assert_eq!(n2, 0);
    }

    #[test]
    fn chunk_window_first_middle_last() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        insert_document_row(
            &c,
            "d1",
            "/a.md",
            "a",
            "markdown",
            "h",
            None,
            "indexed",
            None,
            None,
        )
        .expect("doc");
        insert_chunk_row(&c, "c0", "d1", 0, Some(1), Some(1), "first", 1, "h0", "body", "[]", None)
            .expect("c0");
        insert_chunk_row(&c, "c1", "d1", 1, Some(2), Some(2), "middle", 1, "h1", "body", "[]", None)
            .expect("c1");
        insert_chunk_row(&c, "c2", "d1", 2, Some(3), Some(3), "last", 1, "h2", "body", "[]", None)
            .expect("c2");

        let first = chunk_window(&c, "d1", "c0").expect("q").expect("first");
        assert_eq!(first.highlight, "first");
        assert_eq!(first.text, "first\n\nmiddle");
        assert_eq!(first.page_start, Some(1));
        assert_eq!(first.page_end, Some(1));

        let mid = chunk_window(&c, "d1", "c1").expect("q").expect("mid");
        assert_eq!(mid.highlight, "middle");
        assert_eq!(mid.text, "first\n\nmiddle\n\nlast");
        assert_eq!(mid.page_start, Some(2));

        let last = chunk_window(&c, "d1", "c2").expect("q").expect("last");
        assert_eq!(last.highlight, "last");
        assert_eq!(last.text, "middle\n\nlast");
        assert_eq!(last.page_end, Some(3));
    }

    #[test]
    fn related_documents_ranks_rare_shared_name_above_common() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("v.sqlite");
        let c = open_sqlite(&p).expect("open");
        for (id, title) in [
            ("d0", "Anchor"),
            ("d_rare", "Rare Partner"),
            ("d_common", "Common Partner"),
        ] {
            insert_document_row(
                &c,
                id,
                &format!("/{id}.pdf"),
                title,
                "pdf",
                "hash",
                Some(1),
                "indexed",
                None,
                None,
            )
            .expect("doc");
        }
        insert_chunk_row(&c, "c0", "d0", 0, None, None, "Cora IEEE", 2, "h0", "body", "[]", None)
            .expect("c0");
        insert_chunk_row(&c, "c1", "d_rare", 0, None, None, "Cora", 1, "h1", "body", "[]", None)
            .expect("c1");
        insert_chunk_row(
            &c,
            "c2",
            "d_common",
            0,
            None,
            None,
            "IEEE",
            1,
            "h2",
            "body",
            "[]",
            None,
        )
        .expect("c2");
        c.execute_batch(
            "
            INSERT INTO entities(id, label, name, normalized, doc_count, mention_count)
            VALUES
              ('e_rare', 'person', 'Cora', 'cora', 2, 2),
              ('e_common', 'organization', 'IEEE', 'ieee', 10, 20);
            INSERT INTO mentions(entity_id, chunk_id, document_id, start, end) VALUES
              ('e_rare', 'c0', 'd0', 0, 4),
              ('e_rare', 'c1', 'd_rare', 0, 4),
              ('e_common', 'c0', 'd0', 5, 9),
              ('e_common', 'c2', 'd_common', 0, 4);
            ",
        )
        .expect("entities");
        let related = list_related_documents(&c, "d0").expect("related");
        assert!(related.len() >= 2);
        let ids: Vec<&str> = related
            .iter()
            .filter_map(|v| v.get("id").and_then(|x| x.as_str()))
            .collect();
        let rare_pos = ids.iter().position(|id| *id == "d_rare").expect("rare");
        let common_pos = ids.iter().position(|id| *id == "d_common").expect("common");
        assert!(rare_pos < common_pos);
        assert_eq!(
            related[rare_pos].get("kind").and_then(|v| v.as_str()),
            Some("shares_names")
        );
    }
}
