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
    pub graph_status: String,
    pub graph_error: Option<String>,
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
pub struct GraphExtractionProgress {
    pub job_id: Option<String>,
    pub document_id: String,
    pub phase: String,
    pub detail: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    pub vault_path: Option<String>,
    pub chat_model: String,
    pub embed_model: String,
    #[serde(default = "default_true")]
    pub graph_extraction_enabled: bool,
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
    #[serde(default)]
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamEvent {
    pub message_id: String,
    pub token: String,
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
            node_ids: vec!["n1".into()],
            index: 1,
        };
        let f = ChatFinalResult {
            message_id: "m1".into(),
            citations: vec![c],
            retrieval_mode: "hybrid".into(),
            error: None,
        };
        let j = serde_json::to_string(&f).unwrap();
        let back: ChatFinalResult = serde_json::from_str(&j).unwrap();
        assert_eq!(back.retrieval_mode, "hybrid");
        assert_eq!(back.citations[0].chunk_id, "c1");
    }
}
