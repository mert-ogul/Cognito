use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHealth {
    pub sqlite_ok: bool,
    pub sidecar_reachable: bool,
    pub ollama_model_ready: bool,
    pub vault_configured: bool,
    pub chat_model: String,
    pub embed_model: String,
    /// Chat model weights loaded (`warm_model` completed successfully).
    pub chat_model_warmed: bool,
    /// Pending if warm-up has not finished; set when warm-up fails.
    pub warm_up_pending: bool,
    pub warm_up_error: Option<String>,
    pub ollama_reachable: bool,
    pub missing_models: Vec<String>,
    /// Startup rebuild rewriting embeddings after an index-format bump.
    #[serde(default)]
    pub index_format_updating: bool,
    /// Local checking models (reranker / HHEM) downloaded under app data.
    #[serde(default)]
    pub checking_models_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRow {
    pub id: String,
    pub path: String,
    pub title: String,
    pub file_type: String,
    pub content_hash: String,
    #[serde(default)]
    pub page_count: Option<i64>,
    pub status: String,
    pub error_message: Option<String>,
    pub imported_at: String,
    pub indexed_at: Option<String>,
    pub modified_at: String,
    pub entities_status: String,
    pub entities_error: Option<String>,
    #[serde(default)]
    pub card_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_date: Option<String>,
    #[serde(default)]
    pub card_people: Vec<String>,
    #[serde(default)]
    pub card_organizations: Vec<String>,
    /// null | "ocr" | "no_text" — set for scanned PDFs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPreview {
    pub document_id: String,
    pub text: Option<String>,
    /// Optional passage to scroll/highlight in the preview UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight_snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_end: Option<i64>,
    /// Absolute or vault-relative document path for page rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Array of `{page,x0,y0,x1,y1}` for the cited chunk, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regions: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRenderResult {
    pub png_base64: String,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub job_id: String,
    pub status: String,
    pub total_files: u32,
    pub completed_files: u32,
    pub failed_files: u32,
    pub phase: String,
    pub path: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundProgress {
    pub job_id: Option<String>,
    pub document_id: String,
    pub phase: String,
    pub detail: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_background_model() -> String {
    "qwen3.5:9b".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    pub vault_path: Option<String>,
    pub chat_model: String,
    pub embed_model: String,
    #[serde(default = "default_background_model")]
    pub background_model: String,
    #[serde(default = "default_true")]
    pub summarise_documents: bool,
    #[serde(default = "default_true")]
    pub extract_entities: bool,
    #[serde(default)]
    pub extra_entity_labels: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentCardPayload {
    pub document_id: String,
    #[serde(rename = "type")]
    pub card_type: String,
    pub title: String,
    pub date: Option<String>,
    pub people: Vec<serde_json::Value>,
    pub organizations: Vec<String>,
    pub summary: Vec<serde_json::Value>,
    pub facts: Vec<serde_json::Value>,
    pub fields: Vec<serde_json::Value>,
    pub aliases: Vec<String>,
    pub questions: Vec<String>,
    pub model: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedQuestion {
    pub question: String,
    pub scope_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationItem {
    pub document_id: String,
    pub title: String,
    pub path: String,
    #[serde(default)]
    pub page_start: Option<i64>,
    #[serde(default)]
    pub page_end: Option<i64>,
    pub chunk_id: String,
    pub snippet: String,
    #[serde(default, skip_serializing)]
    pub text: String,
    #[serde(default)]
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regions: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // reserved; grounded chat emits stages + final instead
pub struct ChatStreamEvent {
    pub message_id: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStageEvent {
    pub message_id: String,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<CitationItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub draft: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerSentence {
    pub text: String,
    pub status: String,
    pub sources: Vec<u32>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatTimings {
    #[serde(default)]
    pub retrieval_ms: u64,
    #[serde(default)]
    pub rerank_ms: u64,
    #[serde(default)]
    pub rerank_scored: u32,
    #[serde(default)]
    pub generation_ms: u64,
    #[serde(default)]
    pub check_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSuggestion {
    pub document_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatFinalResult {
    pub message_id: String,
    #[serde(default)]
    pub citations: Vec<CitationItem>,
    pub retrieval_mode: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub not_found: bool,
    #[serde(default)]
    pub sentences: Vec<AnswerSentence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_title: Option<String>,
    #[serde(default)]
    pub scope_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<ChatTimings>,
    #[serde(default)]
    pub suggestions: Vec<ScopeSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPresence {
    pub name: String,
    pub present: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsStatus {
    pub ready: bool,
    pub models: Vec<ModelPresence>,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsProgressEvent {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[cfg(test)]
mod chat_payload_tests {
    use super::*;

    #[test]
    fn chat_final_result_roundtrip() {
        let c = CitationItem {
            document_id: "d1".into(),
            title: "T".into(),
            path: "/p".into(),
            page_start: Some(1),
            page_end: Some(2),
            chunk_id: "c1".into(),
            snippet: "snip".into(),
            text: String::new(),
            node_ids: vec!["n1".into()],
            index: 1,
            regions: None,
        };
        let f = ChatFinalResult {
            message_id: "m1".into(),
            citations: vec![c],
            retrieval_mode: "hybrid".into(),
            error: None,
            not_found: false,
            sentences: vec![],
            scope_title: None,
            scope_ids: vec![],
            timings: None,
            suggestions: vec![],
            trace: None,
        };
        let j = serde_json::to_string(&f).unwrap();
        let back: ChatFinalResult = serde_json::from_str(&j).unwrap();
        assert_eq!(back.retrieval_mode, "hybrid");
        assert_eq!(back.citations[0].chunk_id, "c1");
        assert!(!back.not_found);
        assert!(back.sentences.is_empty());
        assert!(back.scope_ids.is_empty());
    }
}
