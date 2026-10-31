//! Background document cards: summarise indexed documents with a small Ollama model.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::services::ollama_stream;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::Ordering;

const HEAD_WORDS: usize = 2500;
const TAIL_WORDS: usize = 300;
const PASSAGE_TARGET_WORDS: usize = 120;

const CARD_TYPES: &[&str] = &[
    "paper",
    "article",
    "report",
    "contract",
    "agreement",
    "letter",
    "email",
    "cv",
    "invoice",
    "receipt",
    "form",
    "manual",
    "presentation",
    "notes",
    "book",
    "other",
];

const SYSTEM_PROMPT: &str = "You write a short card for one document from numbered passages. \
Every quote must be an exact substring of a numbered passage. \
Choose type from the enum. Title is the document's real title as written. \
Date is ISO when known, else null. \
People are {name, role} when the role is clear. \
Organizations are short names as written. \
Summary is two or three sentences covering what the document is; each item has sentence plus quotes. \
Key facts are at most six concrete claims; each item has fact plus quotes. \
Fields depend on type (name/value with quotes): \
paper uses authors, venue, year, methods, datasets, findings; \
article uses authors, venue, year, findings; \
report uses author, period, findings, recommendations; \
contract and agreement use parties, start date, end date, amounts, obligations, termination; \
letter and email use sender, recipient, purpose, requested action, deadline; \
cv uses person, education, roles, skills; \
invoice and receipt use issuer, customer, total, currency, due date, items; \
manual uses product, tasks covered; \
presentation uses event, topics; \
form uses purpose, parties; \
notes, book and other may use any short useful fields. \
Aliases are short names or abbreviations the document uses for itself. \
Questions are at most three natural follow-ups a reader would ask, answerable from the document.";

fn mock_llm_enabled() -> bool {
    matches!(std::env::var("COGNITO_MOCK_LLM").ok().as_deref(), Some("1"))
}

fn normalize_field_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Allowed field names for a card type. `None` means keep all fields (capped at 5).
pub fn allowed_fields_for_type(card_type: &str) -> Option<&'static [&'static str]> {
    match card_type {
        "paper" => Some(&[
            "authors", "venue", "year", "methods", "datasets", "findings",
        ]),
        "article" => Some(&["authors", "venue", "year", "findings"]),
        "report" => Some(&["author", "period", "findings", "recommendations"]),
        "contract" | "agreement" => Some(&[
            "parties",
            "start date",
            "end date",
            "amounts",
            "obligations",
            "termination",
        ]),
        "letter" | "email" => Some(&[
            "sender",
            "recipient",
            "purpose",
            "requested action",
            "deadline",
        ]),
        "cv" => Some(&["person", "education", "roles", "skills"]),
        "invoice" | "receipt" => Some(&[
            "issuer", "customer", "total", "currency", "due date", "items",
        ]),
        "manual" => Some(&["product", "tasks covered"]),
        "presentation" => Some(&["event", "topics"]),
        "form" | "notes" | "book" | "other" => None,
        _ => None,
    }
}

pub fn filter_fields_for_type(card_type: &str, fields: &[Value]) -> Vec<Value> {
    match allowed_fields_for_type(card_type) {
        None => fields.iter().take(5).cloned().collect(),
        Some(allowed) => {
            let allowed_norm: std::collections::HashSet<String> =
                allowed.iter().map(|a| normalize_field_name(a)).collect();
            fields
                .iter()
                .filter(|f| {
                    f.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| allowed_norm.contains(&normalize_field_name(n)))
                        .unwrap_or(false)
                })
                .cloned()
                .collect()
        }
    }
}

fn item_has_quotes(item: &Value) -> bool {
    item.get("quotes")
        .and_then(|q| q.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
}

/// Drop summary / fact / field items that carry no quotes.
pub fn drop_unquoted_items(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter(|v| item_has_quotes(v))
        .cloned()
        .collect()
}

fn quote_items_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "source": { "type": "integer" },
                "text": { "type": "string" }
            },
            "required": ["source", "text"]
        }
    })
}

fn card_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": { "type": "string", "enum": CARD_TYPES },
            "title": { "type": "string" },
            "date": { "type": ["string", "null"] },
            "people": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "role": { "type": "string" }
                    },
                    "required": ["name"]
                }
            },
            "organizations": { "type": "array", "items": { "type": "string" } },
            "summary": {
                "type": "array",
                "maxItems": 3,
                "items": {
                    "type": "object",
                    "properties": {
                        "sentence": { "type": "string" },
                        "quotes": quote_items_schema()
                    },
                    "required": ["sentence", "quotes"]
                }
            },
            "key_facts": {
                "type": "array",
                "maxItems": 6,
                "items": {
                    "type": "object",
                    "properties": {
                        "fact": { "type": "string" },
                        "quotes": quote_items_schema()
                    },
                    "required": ["fact", "quotes"]
                }
            },
            "fields": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "value": { "type": "string" },
                        "quotes": quote_items_schema()
                    },
                    "required": ["name", "value", "quotes"]
                }
            },
            "aliases": { "type": "array", "items": { "type": "string" } },
            "questions": {
                "type": "array",
                "maxItems": 3,
                "items": { "type": "string" }
            }
        },
        "required": [
            "type", "title", "date", "people", "organizations",
            "summary", "key_facts", "fields", "aliases", "questions"
        ]
    })
}

fn mock_card_json() -> Value {
    json!({
        "type": "paper",
        "title": "Mock Paper Title",
        "date": "2017-01-01",
        "people": [{ "name": "Ada Chen", "role": "author" }],
        "organizations": ["Meridian Labs"],
        "summary": [{
            "sentence": "Ada Chen works at Meridian Labs.",
            "quotes": [{ "source": 1, "text": "Ada Chen works at Meridian Labs." }]
        }],
        "key_facts": [{
            "fact": "Ada Chen works at Meridian Labs.",
            "quotes": [{ "source": 1, "text": "Ada Chen works at Meridian Labs." }]
        }],
        "fields": [
            {
                "name": "authors",
                "value": "Ada Chen",
                "quotes": [{ "source": 1, "text": "Ada Chen works at Meridian Labs." }]
            },
            {
                "name": "venue",
                "value": "Mock Venue",
                "quotes": [{ "source": 1, "text": "Ada Chen works at Meridian Labs." }]
            }
        ],
        "aliases": ["Mock"],
        "questions": ["Who works at Meridian Labs?"]
    })
}

fn take_head_tail_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= HEAD_WORDS + TAIL_WORDS {
        return words.join(" ");
    }
    let head = words[..HEAD_WORDS].join(" ");
    let tail = words[words.len() - TAIL_WORDS..].join(" ");
    format!("{head}\n\n[…]\n\n{tail}")
}

pub fn number_passages(text: &str) -> Vec<(u32, String)> {
    let mut passages = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    let mut count = 0usize;
    for para in text.split("\n\n") {
        let words: Vec<&str> = para.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        if count > 0 && count + words.len() > PASSAGE_TARGET_WORDS {
            passages.push(buf.join(" "));
            buf.clear();
            count = 0;
        }
        buf.extend(words.iter().copied());
        count += words.len();
        if count >= PASSAGE_TARGET_WORDS {
            passages.push(buf.join(" "));
            buf.clear();
            count = 0;
        }
    }
    if !buf.is_empty() {
        passages.push(buf.join(" "));
    }
    if passages.is_empty() {
        passages.push(text.trim().to_string());
    }
    passages
        .into_iter()
        .enumerate()
        .map(|(i, p)| ((i + 1) as u32, p))
        .collect()
}

fn format_passages_for_prompt(passages: &[(u32, String)]) -> String {
    passages
        .iter()
        .map(|(i, t)| format!("[{i}] {t}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn title_looks_like_filename(title: &str, path: &str) -> bool {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem.is_empty() {
        return false;
    }
    let t = title.trim();
    if t.eq_ignore_ascii_case(stem) {
        return true;
    }
    let with_ext = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    t.eq_ignore_ascii_case(with_ext)
}

fn item_text(item: &Value) -> String {
    item.get("sentence")
        .or_else(|| item.get("fact"))
        .or_else(|| item.get("value"))
        .or_else(|| item.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn item_quotes(item: &Value) -> Vec<Value> {
    item.get("quotes")
        .and_then(|q| q.as_array())
        .cloned()
        .unwrap_or_default()
}

fn check_item_supported(
    state: &SharedState,
    passages: &[(u32, String)],
    item: &Value,
) -> CognitoResult<bool> {
    let text = item_text(item);
    let quotes = item_quotes(item);
    if text.trim().is_empty() || quotes.is_empty() {
        return Ok(false);
    }
    let draft = json!({
        "answerable": true,
        "answer": [{ "quotes": quotes, "sentence": text }]
    });
    let passage_vals: Vec<Value> = passages
        .iter()
        .map(|(i, t)| json!({ "index": i, "text": t }))
        .collect();
    let app_data = state.data_dir.to_string_lossy().into_owned();
    let check = state.sidecar.with_session(|s| {
        s.request(
            "check_answer",
            json!({
                "app_data": app_data,
                "passages": passage_vals,
                "draft": draft,
            }),
        )
    })?;
    let sentences = check
        .get("sentences")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(sentences.iter().any(|s| {
        s.get("status")
            .and_then(|st| st.as_str())
            .map(|st| st == "supported" || st == "unchecked")
            .unwrap_or(false)
    }))
}

fn filter_supported_items(
    state: &SharedState,
    passages: &[(u32, String)],
    items: &[Value],
    skip_check: bool,
) -> CognitoResult<Vec<Value>> {
    let quoted = drop_unquoted_items(items);
    if skip_check {
        return Ok(quoted);
    }
    let mut out = Vec::new();
    for item in quoted {
        if check_item_supported(state, passages, &item)? {
            out.push(item);
        }
    }
    Ok(out)
}

fn normalize_card_type(raw: &str) -> String {
    let t = raw.trim().to_lowercase();
    if CARD_TYPES.contains(&t.as_str()) {
        t
    } else {
        "other".into()
    }
}

fn string_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn build_and_store_card(state: &SharedState, document_id: &str) -> CognitoResult<()> {
    let meta = state
        .with_sqlite(|c| sqlite::document_meta_for_card(c, document_id))?
        .ok_or_else(|| CognitoError::Database("document not found".into()))?;
    let (title, path, page_count) = meta;
    let chunks = state.with_sqlite(|c| sqlite::list_chunk_texts_ordered(c, document_id))?;
    let full = chunks.join("\n\n");
    let clipped = take_head_tail_words(&full);
    let passages = number_passages(&clipped);
    let settings = state.with_sqlite(sqlite::load_settings)?;
    let model = settings.background_model.clone();

    let raw = if mock_llm_enabled() {
        mock_card_json()
    } else {
        let file_name = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&path);
        let mut user = format!("Document file name: {file_name}\nCurrent title: {title}\n");
        if let Some(pc) = page_count {
            user.push_str(&format!("Page count: {pc}\n"));
        }
        user.push_str(&format!(
            "\nPassages:\n{}",
            format_passages_for_prompt(&passages)
        ));
        let messages = vec![
            json!({ "role": "system", "content": SYSTEM_PROMPT }),
            json!({ "role": "user", "content": user }),
        ];
        let schema = card_response_schema();
        let state_for_interrupt = state.clone();
        let content = tauri::async_runtime::block_on(async {
            ollama_stream::chat_completion_json_interruptible(&model, messages, schema, || {
                state_for_interrupt.chat_in_flight.load(Ordering::SeqCst) > 0
            })
            .await
        });
        let content = match content {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(CognitoError::Sidecar(e)),
            Err(()) => return Err(CognitoError::InterruptedByChat),
        };
        serde_json::from_str(&content)
            .map_err(|e| CognitoError::Sidecar(format!("card JSON: {e}")))?
    };

    let card_type =
        normalize_card_type(raw.get("type").and_then(|t| t.as_str()).unwrap_or("other"));
    let card_title = raw
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or(&title)
        .trim()
        .to_string();
    let date = raw
        .get("date")
        .and_then(|d| d.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    let people = raw.get("people").cloned().unwrap_or_else(|| json!([]));
    let organizations = string_list(raw.get("organizations").unwrap_or(&json!([])));
    let aliases = string_list(raw.get("aliases").unwrap_or(&json!([])));
    let mut questions = string_list(raw.get("questions").unwrap_or(&json!([])));
    questions.truncate(3);

    let skip_check = mock_llm_enabled();
    let summary_raw = raw
        .get("summary")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let facts_raw = raw
        .get("key_facts")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let fields_raw = raw
        .get("fields")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let fields_typed = filter_fields_for_type(&card_type, &fields_raw);

    let mut summary = filter_supported_items(state, &passages, &summary_raw, skip_check)?;
    summary.truncate(3);
    let mut facts = filter_supported_items(state, &passages, &facts_raw, skip_check)?;
    facts.truncate(6);
    let fields = filter_supported_items(state, &passages, &fields_typed, skip_check)?;

    let people_s = serde_json::to_string(&people).unwrap_or_else(|_| "[]".into());
    let orgs_s = serde_json::to_string(&organizations).unwrap_or_else(|_| "[]".into());
    let summary_s = serde_json::to_string(&summary).unwrap_or_else(|_| "[]".into());
    let facts_s = serde_json::to_string(&facts).unwrap_or_else(|_| "[]".into());
    let fields_s = serde_json::to_string(&fields).unwrap_or_else(|_| "[]".into());
    let aliases_s = serde_json::to_string(&aliases).unwrap_or_else(|_| "[]".into());
    let questions_s = serde_json::to_string(&questions).unwrap_or_else(|_| "[]".into());

    state.with_sqlite(|c| {
        sqlite::upsert_document_card(
            c,
            document_id,
            &card_type,
            &card_title,
            date.as_deref(),
            &people_s,
            &orgs_s,
            &summary_s,
            &facts_s,
            &fields_s,
            &aliases_s,
            &questions_s,
            &model,
        )?;
        if title_looks_like_filename(&title, &path) && !card_title.is_empty() {
            sqlite::update_document_title(c, document_id, &card_title)?;
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AppState, SharedState};
    use crate::db::sqlite;
    use crate::errors::CognitoError;
    use crate::sidecar::SidecarPool;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn drops_unquoted_facts_and_fields() {
        let items = vec![
            json!({ "sentence": "kept", "quotes": [{ "source": 1, "text": "kept" }] }),
            json!({ "fact": "dropped", "quotes": [] }),
            json!({ "name": "venue", "value": "x", "quotes": [{ "source": 1, "text": "x" }] }),
            json!({ "name": "doi", "value": "y" }),
        ];
        let kept = drop_unquoted_items(&items);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0]["sentence"], "kept");
        assert_eq!(kept[1]["name"], "venue");
    }

    #[test]
    fn fields_kept_per_type() {
        let fields = vec![
            json!({ "name": "venue", "value": "ICLR", "quotes": [{ "source": 1, "text": "ICLR" }] }),
            json!({ "name": "sender", "value": "Hi", "quotes": [{ "source": 1, "text": "Hi" }] }),
            json!({ "name": "Authors", "value": "Ada", "quotes": [{ "source": 1, "text": "Ada" }] }),
            json!({ "name": "Year", "value": "2017", "quotes": [{ "source": 1, "text": "2017" }] }),
        ];
        let paper = filter_fields_for_type("paper", &fields);
        assert_eq!(paper.len(), 3);
        assert!(paper.iter().all(|f| {
            let n = normalize_field_name(f["name"].as_str().unwrap_or(""));
            matches!(n.as_str(), "venue" | "authors" | "year")
        }));
        let email = filter_fields_for_type("email", &fields);
        assert_eq!(email.len(), 1);
        assert_eq!(email[0]["name"], "sender");
        let other = filter_fields_for_type("other", &fields);
        assert_eq!(other.len(), 4);
    }

    #[test]
    fn card_ollama_drops_within_500ms_when_chat_starts() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::remove_var("COGNITO_MOCK_LLM");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                thread::sleep(Duration::from_secs(30));
                let body = br#"{"message":{"content":"{}"}}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            }
        });
        std::env::set_var("OLLAMA_HOST", format!("http://{addr}"));

        let data = tempfile::tempdir().expect("tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
            data.path().to_path_buf(),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "summarise_documents", "true")?;
                sqlite::upsert_setting(c, "background_model", "test-model")?;
                sqlite::insert_document_row(
                    c,
                    "doc-card-drop",
                    "/tmp/card-drop.md",
                    "Card Drop",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                sqlite::insert_chunk_row(
                    c,
                    "c-card-drop",
                    "doc-card-drop",
                    0,
                    None,
                    None,
                    "Body text for a slow card summarisation request.",
                    8,
                    "h",
                    "body",
                    "[]",
                    None,
                )?;
                Ok(())
            })
            .expect("seed");

        let state_card = state.clone();
        let card = thread::spawn(move || build_and_store_card(&state_card, "doc-card-drop"));
        thread::sleep(Duration::from_millis(150));
        let chat_started = Instant::now();
        state.chat_in_flight.fetch_add(1, Ordering::SeqCst);
        let result = card.join().expect("card join");
        let elapsed = chat_started.elapsed();
        state.chat_in_flight.fetch_sub(1, Ordering::SeqCst);
        std::env::remove_var("OLLAMA_HOST");
        drop(server);
        assert!(
            matches!(result, Err(CognitoError::InterruptedByChat)),
            "expected InterruptedByChat, got {result:?}"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "card call took {elapsed:?} after chat started"
        );
    }
}
