//! Stored chats: conversations and chat_messages.

use crate::errors::CognitoResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListItem {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatMessage {
    pub id: String,
    pub conversation_id: String,
    pub position: i64,
    pub role: String,
    pub status: String,
    pub content: String,
    pub payload: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<StoredChatMessage>,
}

pub fn conversation_title(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= 60 {
        return collapsed;
    }
    let mut cut = String::new();
    for word in collapsed.split_whitespace() {
        let next = if cut.is_empty() {
            word.to_string()
        } else {
            format!("{cut} {word}")
        };
        if next.chars().count() > 60 {
            break;
        }
        cut = next;
    }
    if cut.is_empty() {
        let truncated: String = collapsed.chars().take(60).collect();
        return format!("{truncated}…");
    }
    format!("{cut}…")
}

pub fn list_conversations(conn: &Connection) -> CognitoResult<Vec<ConversationListItem>> {
    let mut stmt =
        conn.prepare("SELECT id, title, updated_at FROM conversations ORDER BY updated_at DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(ConversationListItem {
            id: r.get(0)?,
            title: r.get(1)?,
            updated_at: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_conversation(conn: &Connection, id: &str) -> CognitoResult<Option<ConversationDetail>> {
    let meta = conn
        .query_row(
            "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((cid, title, created_at, updated_at)) = meta else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, position, role, status, content, payload, created_at
         FROM chat_messages WHERE conversation_id = ?1 ORDER BY position ASC",
    )?;
    let rows = stmt.query_map([id], |r| {
        let payload_raw: Option<String> = r.get(6)?;
        let payload = payload_raw.and_then(|s| serde_json::from_str(&s).ok());
        Ok(StoredChatMessage {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            position: r.get(2)?,
            role: r.get(3)?,
            status: r.get(4)?,
            content: r.get(5)?,
            payload,
            created_at: r.get(7)?,
        })
    })?;
    let mut messages = Vec::new();
    for row in rows {
        messages.push(row?);
    }
    Ok(Some(ConversationDetail {
        id: cid,
        title,
        created_at,
        updated_at,
        messages,
    }))
}

pub fn create_conversation(conn: &Connection, title: &str) -> CognitoResult<ConversationListItem> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let title = if title.trim().is_empty() {
        "New chat".to_string()
    } else {
        conversation_title(title)
    };
    conn.execute(
        "INSERT INTO conversations(id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, title, now, now],
    )?;
    Ok(ConversationListItem {
        id,
        title,
        updated_at: now,
    })
}

pub fn conversation_exists(conn: &Connection, id: &str) -> CognitoResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM conversations WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Append a user message and a pending assistant row. Returns the user message id.
pub fn append_turn(
    conn: &Connection,
    conversation_id: &str,
    user_id: &str,
    user_text: &str,
    assistant_id: &str,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let next_pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM chat_messages WHERE conversation_id = ?1",
        [conversation_id],
        |r| r.get(0),
    )?;
    let msg_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM chat_messages WHERE conversation_id = ?1",
        [conversation_id],
        |r| r.get(0),
    )?;
    if msg_count == 0 {
        let title = conversation_title(user_text);
        conn.execute(
            "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now, conversation_id],
        )?;
    } else {
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        )?;
    }
    conn.execute(
        "INSERT INTO chat_messages(id, conversation_id, position, role, status, content, payload, created_at)
         VALUES (?1, ?2, ?3, 'user', 'done', ?4, NULL, ?5)",
        params![user_id, conversation_id, next_pos, user_text, now],
    )?;
    conn.execute(
        "INSERT INTO chat_messages(id, conversation_id, position, role, status, content, payload, created_at)
         VALUES (?1, ?2, ?3, 'assistant', 'pending', '', NULL, ?4)",
        params![assistant_id, conversation_id, next_pos + 1, now],
    )?;
    Ok(())
}

pub fn finish_assistant_message(
    conn: &Connection,
    message_id: &str,
    status: &str,
    content: &str,
    payload: Option<&Value>,
) -> CognitoResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let payload_str = payload.map(|v| v.to_string());
    let conversation_id: String = conn.query_row(
        "SELECT conversation_id FROM chat_messages WHERE id = ?1",
        [message_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE chat_messages SET status = ?1, content = ?2, payload = ?3 WHERE id = ?4",
        params![status, content, payload_str, message_id],
    )?;
    conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    )?;
    Ok(())
}

pub fn delete_conversation(conn: &Connection, id: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
    Ok(())
}

pub fn delete_all_conversations(conn: &Connection) -> CognitoResult<()> {
    conn.execute("DELETE FROM conversations", [])?;
    Ok(())
}

pub fn mark_pending_interrupted(conn: &Connection) -> CognitoResult<usize> {
    let text = "This answer was interrupted.";
    let payload = serde_json::json!({ "error": text }).to_string();
    let n = conn.execute(
        "UPDATE chat_messages
         SET status = 'error', content = ?1, payload = ?2
         WHERE status = 'pending'",
        params![text, payload],
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory() -> Connection {
        let c = Connection::open_in_memory().expect("mem");
        c.execute_batch("PRAGMA foreign_keys = ON;").expect("fk");
        super::super::migrate(&c).expect("migrate");
        c
    }

    #[test]
    fn create_append_finish_list_delete_and_pending() {
        let c = memory();
        let a = create_conversation(&c, "First question here").expect("create a");
        let b = create_conversation(&c, "Second chat").expect("create b");
        assert_eq!(a.title, "First question here");

        append_turn(&c, &a.id, "u1", "First question here", "m1").expect("append");
        finish_assistant_message(
            &c,
            "m1",
            "done",
            "Answer text.",
            Some(&json!({
                "citations": [],
                "sentences": [{"text": "Answer text.", "status": "supported", "sources": []}],
                "notFound": false,
                "scopeTitle": null,
                "suggestions": [],
                "error": null
            })),
        )
        .expect("finish");

        // Newer update first: touch b after a finished so b is newest.
        append_turn(&c, &b.id, "u2", "Second chat", "m2").expect("append b");

        let list = list_conversations(&c).expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, b.id);
        assert_eq!(list[1].id, a.id);

        let loaded = get_conversation(&c, &a.id).expect("get").expect("some");
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, "user");
        assert_eq!(loaded.messages[1].status, "done");
        assert_eq!(loaded.messages[1].content, "Answer text.");

        append_turn(&c, &b.id, "u3", "follow up", "m3").expect("pending");
        assert_eq!(mark_pending_interrupted(&c).expect("interrupt"), 2);
        let b_loaded = get_conversation(&c, &b.id).expect("get b").expect("some");
        let pending_like: Vec<_> = b_loaded
            .messages
            .iter()
            .filter(|m| m.role == "assistant")
            .collect();
        assert!(pending_like.iter().all(|m| m.status == "error"));
        assert!(pending_like
            .iter()
            .all(|m| m.content == "This answer was interrupted."));

        delete_conversation(&c, &a.id).expect("delete");
        assert!(get_conversation(&c, &a.id).expect("get").is_none());
        let msgs: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE conversation_id = ?1",
                [&a.id],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(msgs, 0);

        delete_all_conversations(&c).expect("delete all");
        assert!(list_conversations(&c).expect("list").is_empty());
    }

    #[test]
    fn title_cuts_at_word_boundary() {
        let long =
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen";
        let t = conversation_title(long);
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 61);
        assert!(!t.ends_with("fourteen…"));
    }
}
