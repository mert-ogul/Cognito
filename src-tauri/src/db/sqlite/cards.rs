use crate::errors::CognitoResult;
use crate::models::{DocumentCardPayload, SuggestedQuestion};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;

use super::settings::{parse_json_string_array, parse_json_value_array};

pub(crate) fn page1_head_text(conn: &Connection, document_id: &str) -> CognitoResult<String> {
    let mut stmt = conn.prepare(
        "SELECT text FROM chunks
             WHERE document_id = ?1
               AND (page_start IS NULL OR page_start <= 1)
             ORDER BY ordinal",
    )?;
    let mut out = String::new();
    let rows = stmt.query_map([document_id], |r| r.get::<_, String>(0))?;
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

pub(crate) fn json_name_list(raw: &str) -> Vec<String> {
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
        .replace(['_', '-'], " ");
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
    let mut stmt =
        conn.prepare("SELECT text FROM chunks WHERE document_id = ?1 ORDER BY ordinal ASC")?;
    let rows = stmt.query_map(params![doc_id], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_suggested_questions(conn: &Connection) -> CognitoResult<Vec<SuggestedQuestion>> {
    let mut stmt = conn.prepare(
        "SELECT c.document_id, c.questions, c.title, c.people, c.organizations, d.path
             FROM document_cards c
             INNER JOIN documents d ON d.id = c.document_id
             WHERE d.status = 'indexed' AND d.card_status = 'ok'
             ORDER BY d.indexed_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
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
        let (doc_id, questions_raw, title, people_raw, orgs_raw, path) = row?;
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
