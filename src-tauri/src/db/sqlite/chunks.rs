use crate::errors::CognitoResult;
use rusqlite::{params, Connection, OptionalExtension};

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
        .query_row("SELECT path FROM documents WHERE id = ?1", [id], |r| {
            r.get::<_, String>(0)
        })
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
    let mut stmt = conn.prepare(
        "SELECT path FROM documents WHERE status = 'failed' ORDER BY imported_at LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| r.get(0))?;
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
    let mut stmt =
        conn.prepare("SELECT id FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC")?;
    let rows = stmt.query_map(params![doc_id], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
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
