use crate::errors::CognitoResult;
use crate::models::SettingsPayload;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

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
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    Ok(row)
}

pub fn clear_setting(conn: &Connection, key: &str) -> CognitoResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
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
    let background = get_setting(conn, "background_model")?.unwrap_or_else(|| chat.clone());
    let summarise = get_setting(conn, "summarise_documents")?.unwrap_or_else(|| "true".to_string());
    let extract = get_setting(conn, "extract_entities")?.unwrap_or_else(|| "true".to_string());
    let extra_labels = get_setting(conn, "extra_entity_labels")?.unwrap_or_else(String::new);
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

pub(crate) fn parse_json_string_array(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s),
            Value::Object(map) => map.get("name").and_then(|n| n.as_str()).map(str::to_string),
            _ => None,
        })
        .collect()
}

pub(crate) fn parse_json_value_array(raw: &str) -> Vec<Value> {
    serde_json::from_str(raw).unwrap_or_else(|_| Vec::new())
}
