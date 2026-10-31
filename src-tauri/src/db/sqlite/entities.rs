use crate::errors::CognitoResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

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
    let n = conn.execute(
        "UPDATE documents SET entities_status = 'pending', entities_error = NULL, modified_at = ?1
             WHERE status = 'indexed' AND entities_status = 'failed'",
        params![now],
    )?;
    Ok(n)
}

pub fn requeue_all_entities(conn: &Connection) -> CognitoResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn.execute(
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

pub fn prune_entities(conn: &Connection) -> CognitoResult<()> {
    conn.execute_batch(
        "UPDATE entities SET
  mention_count = (SELECT COUNT(*) FROM mentions m WHERE m.entity_id = entities.id),
  doc_count = (SELECT COUNT(DISTINCT m.document_id) FROM mentions m WHERE m.entity_id = entities.id);
DELETE FROM entities WHERE mention_count = 0;",
    )?;
    Ok(())
}

pub fn clear_document_entity_rows(conn: &Connection, document_id: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM mentions WHERE document_id = ?1", [document_id])?;
    conn.execute(
        "DELETE FROM document_links WHERE source_id = ?1 OR target_id = ?1",
        [document_id],
    )?;
    prune_entities(conn)?;
    Ok(())
}

pub fn empty_connection_tables(conn: &Connection) -> CognitoResult<()> {
    conn.execute("DELETE FROM entities", [])?;
    let _ = conn.execute("DELETE FROM topics", []);
    let _ = conn.execute("DELETE FROM document_points", []);
    Ok(())
}

pub fn list_entities_index(conn: &Connection) -> CognitoResult<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, normalized, label, mention_count, doc_count
             FROM entities ORDER BY mention_count DESC, name ASC LIMIT 500",
    )?;
    let rows = stmt.query_map([], |r| {
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

    let mut alias_stmt =
        conn.prepare("SELECT alias FROM entity_aliases WHERE entity_id = ?1 ORDER BY alias")?;
    let aliases: Vec<String> = alias_stmt
        .query_map([entity_id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut doc_stmt = conn.prepare(
        "SELECT DISTINCT d.id, d.title, d.path
             FROM mentions m
             JOIN documents d ON d.id = m.document_id
             WHERE m.entity_id = ?1
             ORDER BY d.title COLLATE NOCASE, d.id",
    )?;
    let doc_rows: Vec<(String, String, String)> = doc_stmt
        .query_map([entity_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut passage_stmt = conn.prepare(
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

    let mut co_stmt = conn.prepare(
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

pub(crate) fn mention_snippet(text: &str, start: i64, end: i64) -> String {
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
        snippet.insert(0, '…');
    }
    if to < chars.len() {
        snippet.push('…');
    }
    snippet
}

pub(crate) fn related_link_kind_label(kind: &str) -> Option<&'static str> {
    match kind {
        "mentions" => Some("mentions this document"),
        "similar" => Some("similar content"),
        _ => None,
    }
}

pub(crate) fn entity_label_rank(label: &str) -> i32 {
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

    let mut share_stmt = conn.prepare(
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
    let share_rows = share_stmt.query_map([document_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;
    for row in share_rows {
        let (oid, title, name, label, doc_count) = row?;
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

    let mut mention_stmt = conn.prepare(
        "SELECT l.source_id, IFNULL(d.title, '')
             FROM document_links l
             JOIN documents d ON d.id = l.source_id
             WHERE l.kind = 'mentions' AND l.target_id = ?1 AND d.status = 'indexed'",
    )?;
    let mention_rows = mention_stmt.query_map([document_id], |r| {
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

    let mut similar_stmt = conn.prepare(
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
    let similar_rows = similar_stmt.query_map([document_id], |r| {
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

    let mut card_stmt = conn.prepare(
        "SELECT c.document_id, c.title, c.date
             FROM document_cards c
             WHERE c.date IS NOT NULL AND TRIM(c.date) != ''
             ORDER BY c.date ASC, c.title COLLATE NOCASE ASC
             LIMIT 500",
    )?;
    let card_rows = card_stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in card_rows {
        let (document_id, title, date) = row?;
        items.push(serde_json::json!({
            "kind": "document",
            "id": document_id,
            "title": title,
            "date": date,
            "documentId": document_id,
        }));
    }

    let mut date_stmt = conn.prepare(
        "SELECT id, name, normalized
             FROM entities
             WHERE lower(label) = 'date'
               AND normalized IS NOT NULL AND TRIM(normalized) != ''
             ORDER BY normalized ASC, name COLLATE NOCASE ASC
             LIMIT 500",
    )?;
    let date_rows = date_stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in date_rows {
        let (entity_id, name, date) = row?;
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
        Ok(
            conn.query_row("SELECT name FROM entities WHERE id = ?1", [id], |r| {
                r.get(0)
            })?,
        )
    };
    Ok((name_of(a)?, name_of(b)?))
}

pub fn pending_entities_or_cards_remain(conn: &Connection) -> CognitoResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM documents WHERE status = 'indexed' AND (
                entities_status IN ('pending', 'running')
                OR card_status IN ('pending', 'running')
             )",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}
