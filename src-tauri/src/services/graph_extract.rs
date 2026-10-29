//! Structured graph extraction via Ollama `/api/chat` (non-streaming JSON schema).

use crate::errors::{CognitoError, CognitoResult};
use crate::services::ollama_stream::{ollama_base_url, CHAT_CONTEXT_TOKENS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ENTITIES: usize = 8;
const MAX_RELATIONS: usize = 10;
const NUM_PREDICT: u32 = 800;

const ALLOWED_KINDS: &[&str] = &[
    "concept",
    "method",
    "model",
    "dataset",
    "person",
    "organization",
    "work",
    "place",
];

const SYSTEM_PROMPT: &str = "You map what a passage is about. List at most 8 of the main ideas, methods, models, datasets, people, organizations and works the passage discusses. Give each a short name of one to five words as written in the text, and prefer the full name to an abbreviation. Skip author affiliations, funding and grant details, journal and conference names, figure, table and equation labels, section titles, citations, IDs, numbers and dates. Add relations the passage states between two listed items, as a short lowercase verb phrase such as \"extends\", \"is evaluated on\" or \"was proposed by\". If the passage is only references, acknowledgements or front matter, return empty lists.";

const RETRY_EXTRA: &str = " List at most 4 items.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEntity {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRelation {
    pub source: String,
    pub target: String,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkGraph {
    pub entities: Vec<GraphEntity>,
    pub relations: Vec<GraphRelation>,
}

fn response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "entities": {
                "type": "array",
                "maxItems": MAX_ENTITIES,
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "kind": {
                            "type": "string",
                            "enum": ALLOWED_KINDS
                        }
                    },
                    "required": ["name", "kind"]
                }
            },
            "relations": {
                "type": "array",
                "maxItems": MAX_RELATIONS,
                "items": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string" },
                        "target": { "type": "string" },
                        "relation": { "type": "string" }
                    },
                    "required": ["source", "target", "relation"]
                }
            }
        },
        "required": ["entities", "relations"]
    })
}

/// Normalize for dedupe: trim, lowercase, collapse whitespace (matches sidecar intent).
pub fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn word_count(name: &str) -> usize {
    name.split_whitespace().count()
}

fn has_letter(name: &str) -> bool {
    name.chars().any(|c| c.is_alphabetic())
}

fn looks_like_label_prefix(name: &str) -> bool {
    let first = name
        .split(|c: char| c.is_whitespace() || c == '.' || c == ':' || c == '(')
        .next()
        .unwrap_or("")
        .to_lowercase();
    matches!(
        first.as_str(),
        "fig" | "figure" | "table" | "eq" | "equation" | "section" | "appendix"
    )
}

fn looks_like_arxiv_id(name: &str) -> bool {
    // e.g. 1609.02907 or 1609.02907v4
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i + 9 <= chars.len() {
        if chars[i].is_ascii_digit()
            && chars[i + 1].is_ascii_digit()
            && chars[i + 2].is_ascii_digit()
            && chars[i + 3].is_ascii_digit()
            && chars[i + 4] == '.'
            && chars[i + 5].is_ascii_digit()
            && chars[i + 6].is_ascii_digit()
            && chars[i + 7].is_ascii_digit()
            && chars[i + 8].is_ascii_digit()
        {
            return true;
        }
        i += 1;
    }
    false
}

fn looks_like_doi(name: &str) -> bool {
    let low = name.to_lowercase();
    if let Some(rest) = low.strip_prefix("10.") {
        let mut digits = 0usize;
        for c in rest.chars() {
            if c.is_ascii_digit() {
                digits += 1;
            } else if c == '/' && digits >= 4 {
                return true;
            } else {
                break;
            }
        }
    }
    low.contains("doi:")
}

fn looks_like_id_or_url(name: &str) -> bool {
    let low = name.to_lowercase();
    if low.contains("arxiv")
        || low.contains("http://")
        || low.contains("https://")
        || looks_like_doi(name)
        || looks_like_arxiv_id(name)
    {
        return true;
    }
    if low.contains('@') && low.contains('.') {
        return true;
    }
    // Grant/code tokens: letters and digits only, no spaces, mixed both.
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    if name.split_whitespace().count() == 1
        && compact.len() >= 5
        && compact.chars().all(|c| c.is_ascii_alphanumeric())
        && compact.chars().any(|c| c.is_ascii_alphabetic())
        && compact.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }
    false
}

fn should_drop_entity_name(name: &str) -> bool {
    if name.chars().count() > 60 || word_count(name) > 6 {
        return true;
    }
    if !has_letter(name) {
        return true;
    }
    if looks_like_label_prefix(name) {
        return true;
    }
    if looks_like_id_or_url(name) {
        return true;
    }
    false
}

/// Validate and clean a model reply into a ChunkGraph.
pub fn validate_chunk_graph(raw: &Value) -> CognitoResult<ChunkGraph> {
    let entities_val = raw
        .get("entities")
        .ok_or_else(|| CognitoError::Sidecar("graph extract: missing entities".into()))?;
    let relations_val = raw
        .get("relations")
        .ok_or_else(|| CognitoError::Sidecar("graph extract: missing relations".into()))?;
    let entities_arr = entities_val
        .as_array()
        .ok_or_else(|| CognitoError::Sidecar("graph extract: entities must be an array".into()))?;
    let relations_arr = relations_val
        .as_array()
        .ok_or_else(|| CognitoError::Sidecar("graph extract: relations must be an array".into()))?;

    let mut entities: Vec<GraphEntity> = Vec::new();
    let mut seen_norm: HashSet<String> = HashSet::new();
    let mut name_by_norm: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for item in entities_arr.iter() {
        if entities.len() >= MAX_ENTITIES {
            break;
        }
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() || should_drop_entity_name(&name) {
            continue;
        }
        let norm = normalize_name(&name);
        if norm.is_empty() || seen_norm.contains(&norm) {
            continue;
        }
        let kind_raw = item
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("concept")
            .trim()
            .to_lowercase();
        let kind = if ALLOWED_KINDS.contains(&kind_raw.as_str()) {
            kind_raw
        } else {
            "concept".to_string()
        };
        seen_norm.insert(norm.clone());
        name_by_norm.insert(norm, name.clone());
        entities.push(GraphEntity { name, kind });
    }

    let mut relations: Vec<GraphRelation> = Vec::new();
    for item in relations_arr.iter().take(MAX_RELATIONS) {
        let source = item
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let target = item
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let relation = item
            .get("relation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if source.is_empty() || target.is_empty() || relation.is_empty() {
            continue;
        }
        let sn = normalize_name(&source);
        let tn = normalize_name(&target);
        if sn == tn {
            continue;
        }
        let Some(src_name) = name_by_norm.get(&sn) else {
            continue;
        };
        let Some(tgt_name) = name_by_norm.get(&tn) else {
            continue;
        };
        relations.push(GraphRelation {
            source: src_name.clone(),
            target: tgt_name.clone(),
            relation: relation.chars().take(256).collect(),
        });
    }

    Ok(ChunkGraph {
        entities,
        relations,
    })
}

fn mock_chunk_graph() -> ChunkGraph {
    ChunkGraph {
        entities: vec![
            GraphEntity {
                name: "Mock Alpha".into(),
                kind: "person".into(),
            },
            GraphEntity {
                name: "Mock Beta".into(),
                kind: "organization".into(),
            },
        ],
        relations: vec![GraphRelation {
            source: "Mock Alpha".into(),
            target: "Mock Beta".into(),
            relation: "works at".into(),
        }],
    }
}

pub fn ollama_unreachable(err: &CognitoError) -> bool {
    let s = err.to_string().to_lowercase();
    s.contains("error sending request")
        || s.contains("connection refused")
        || s.contains("connect error")
        || s.contains("tcp connect")
        || s.contains("dns error")
        || s.contains("failed to connect")
        || (s.contains("timed out") && s.contains("ollama"))
}

async fn call_ollama(
    chat_model: &str,
    text: &str,
    system: &str,
) -> CognitoResult<(Value, Option<String>)> {
    let client = reqwest::Client::builder()
        .timeout(EXTRACT_TIMEOUT)
        .build()
        .map_err(|e| CognitoError::Sidecar(e.to_string()))?;
    let url = format!("{}/api/chat", ollama_base_url().trim_end_matches('/'));
    let body = json!({
        "model": chat_model,
        "stream": false,
        "keep_alive": -1,
        "format": response_schema(),
        "options": {
            "num_ctx": CHAT_CONTEXT_TOKENS,
            "num_predict": NUM_PREDICT,
            "temperature": 0
        },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": text }
        ]
    });
    let res = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| CognitoError::Sidecar(format!("Ollama graph extract failed: {e}")))?;
    let status = res.status();
    let text_body = res
        .text()
        .await
        .map_err(|e| CognitoError::Sidecar(e.to_string()))?;
    if !status.is_success() {
        return Err(CognitoError::Sidecar(format!(
            "Ollama returned {}: {}",
            status,
            text_body.chars().take(200).collect::<String>()
        )));
    }
    let v: Value = serde_json::from_str(&text_body)
        .map_err(|e| CognitoError::Sidecar(format!("graph extract parse: {e}")))?;
    let done_reason = v
        .get("done_reason")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Ok((v, done_reason))
}

fn graph_from_response(v: &Value) -> CognitoResult<ChunkGraph> {
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| CognitoError::Sidecar("graph extract: empty message content".into()))?;
    let parsed: Value = serde_json::from_str(content)
        .map_err(|e| CognitoError::Sidecar(format!("graph extract JSON: {e}")))?;
    validate_chunk_graph(&parsed)
}

pub async fn extract_chunk(chat_model: &str, text: &str) -> CognitoResult<ChunkGraph> {
    if std::env::var("COGNITO_MOCK_EXTRACT").ok().as_deref() == Some("1") {
        return Ok(mock_chunk_graph());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(ChunkGraph {
            entities: vec![],
            relations: vec![],
        });
    }

    let (v, done_reason) = call_ollama(chat_model, trimmed, SYSTEM_PROMPT).await?;
    let truncated = done_reason.as_deref() == Some("length");
    match graph_from_response(&v) {
        Ok(g) if !truncated => Ok(g),
        first => {
            let retry_system = format!("{SYSTEM_PROMPT}{RETRY_EXTRA}");
            let (v2, _) = call_ollama(chat_model, trimmed, &retry_system).await?;
            match graph_from_response(&v2) {
                Ok(g) => Ok(g),
                Err(e) => match first {
                    Ok(g) => Ok(g),
                    Err(_) => Err(e),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drops_empty_and_duplicate_names() {
        let raw = json!({
            "entities": [
                { "name": "  Ada Chen  ", "kind": "person" },
                { "name": "ada chen", "kind": "person" },
                { "name": "   ", "kind": "place" },
                { "name": "Meridian Labs", "kind": "organization" }
            ],
            "relations": []
        });
        let g = validate_chunk_graph(&raw).expect("ok");
        assert_eq!(g.entities.len(), 2);
        assert_eq!(g.entities[0].name, "Ada Chen");
        assert_eq!(g.entities[1].name, "Meridian Labs");
    }

    #[test]
    fn drops_relations_to_unknown_entities() {
        let raw = json!({
            "entities": [
                { "name": "Ada", "kind": "person" },
                { "name": "Meridian", "kind": "organization" }
            ],
            "relations": [
                { "source": "Ada", "target": "Meridian", "relation": "works at" },
                { "source": "Ada", "target": "Unknown", "relation": "knows" },
                { "source": "Ghost", "target": "Meridian", "relation": "funds" }
            ]
        });
        let g = validate_chunk_graph(&raw).expect("ok");
        assert_eq!(g.relations.len(), 1);
        assert_eq!(g.relations[0].relation, "works at");
        assert_eq!(g.relations[0].source, "Ada");
        assert_eq!(g.relations[0].target, "Meridian");
    }

    #[test]
    fn drops_junk_entity_names() {
        let raw = json!({
            "entities": [
                { "name": "Graph convolutional network", "kind": "method" },
                { "name": "Fig. 1(c)", "kind": "concept" },
                { "name": "LX045446", "kind": "organization" },
                { "name": "arXiv:1609.02907", "kind": "work" },
                { "name": "https://example.com/x", "kind": "work" },
                { "name": "a@b.com", "kind": "person" },
                { "name": "12345", "kind": "concept" },
                { "name": "This is a whole abstract sentence about graphs and learning", "kind": "concept" },
                { "name": "Table 2", "kind": "concept" },
                { "name": "GAT", "kind": "model" },
            ],
            "relations": [
                { "source": "GAT", "target": "GAT", "relation": "is" },
                { "source": "GAT", "target": "Graph convolutional network", "relation": "extends" },
            ]
        });
        let g = validate_chunk_graph(&raw).expect("ok");
        let names: Vec<_> = g.entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Graph convolutional network"));
        assert!(names.contains(&"GAT"));
        assert!(!names.iter().any(|n| n.contains("Fig")));
        assert!(!names.iter().any(|n| *n == "LX045446"));
        assert!(!names.iter().any(|n| n.contains("arXiv")));
        assert!(!names.iter().any(|n| n.contains("http")));
        assert!(!names.iter().any(|n| n.contains('@')));
        assert_eq!(g.relations.len(), 1);
        assert_eq!(g.relations[0].relation, "extends");
    }

    #[test]
    fn malformed_json_is_error_not_panic() {
        let raw = json!({ "entities": "nope", "relations": [] });
        let err = validate_chunk_graph(&raw).expect_err("must fail");
        assert!(err.to_string().contains("entities must be an array"));

        let missing = json!({ "relations": [] });
        let err2 = validate_chunk_graph(&missing).expect_err("must fail");
        assert!(err2.to_string().contains("missing entities"));
    }

    #[test]
    fn mock_extract_skips_ollama() {
        std::env::set_var("COGNITO_MOCK_EXTRACT", "1");
        let g = tauri::async_runtime::block_on(extract_chunk("phi4-mini", "ignored")).expect("mock");
        assert_eq!(g.entities.len(), 2);
        assert_eq!(g.relations.len(), 1);
        std::env::remove_var("COGNITO_MOCK_EXTRACT");
    }

    #[test]
    fn ollama_unreachable_detects_connection_errors() {
        assert!(ollama_unreachable(&CognitoError::Sidecar(
            "Ollama graph extract failed: error sending request for url".into()
        )));
        assert!(ollama_unreachable(&CognitoError::Sidecar(
            "connection refused".into()
        )));
        assert!(!ollama_unreachable(&CognitoError::Sidecar(
            "graph extract JSON: expected value".into()
        )));
    }
}
