//! Structured graph extraction via Ollama `/api/chat` (non-streaming JSON schema).

use crate::errors::{CognitoError, CognitoResult};
use crate::services::ollama_stream::{ollama_base_url, CHAT_CONTEXT_TOKENS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ENTITIES: usize = 10;
const MAX_RELATIONS: usize = 12;

const ALLOWED_KINDS: &[&str] = &[
    "person",
    "organization",
    "place",
    "program",
    "concept",
    "event",
    "document",
];

const SYSTEM_PROMPT: &str = "List the people, organizations, places, programs, concepts, events and documents named in the passage, using names as written. Leave out numbers, IDs, dates, email addresses, phone numbers, street addresses and URLs. Add a relation only between two listed entities, as a short lowercase verb phrase of one to four words. If there is nothing worth listing, return empty arrays.";

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

    for item in entities_arr.iter().take(MAX_ENTITIES) {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
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
            "num_predict": 400,
            "temperature": 0
        },
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": trimmed }
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
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| CognitoError::Sidecar("graph extract: empty message content".into()))?;
    let parsed: Value = serde_json::from_str(content)
        .map_err(|e| CognitoError::Sidecar(format!("graph extract JSON: {e}")))?;
    validate_chunk_graph(&parsed)
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
}
