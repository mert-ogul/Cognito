//! Orchestrate sidecar retrieval + grounded JSON chat for `send_chat_query`.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::events::{CHAT_FINAL, CHAT_STAGE};
use crate::models::{
    AnswerSentence, ChatFinalResult, ChatStageEvent, ChatTimings, ChatTurn, CitationItem,
    ScopeSuggestion,
};
use crate::services::ollama_stream;
use crate::vault;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

const SYSTEM_PROMPT: &str = "Answer only from the numbered passages. First copy up to four short, exact sentences you rely on. Then answer the question in plain, natural sentences built only from those quotes, and list the quotes each sentence uses. Write formulas in LaTeX between $ or $$. If the passages don't answer the question, set answerable to false and leave both lists empty.";

fn answer_format_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "answerable": { "type": "boolean" },
            "quotes": {
                "type": "array",
                "maxItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "integer" },
                        "text": { "type": "string" }
                    },
                    "required": ["source", "text"]
                }
            },
            "answer": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "sentence": { "type": "string" },
                        "quotes": {
                            "type": "array",
                            "items": { "type": "integer" }
                        }
                    },
                    "required": ["sentence", "quotes"]
                }
            }
        },
        "required": ["answerable", "quotes", "answer"]
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftQuote {
    pub source: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftAnswerItem {
    pub sentence: String,
    pub quotes: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedDraft {
    pub answerable: bool,
    pub quotes: Vec<DraftQuote>,
    pub answer: Vec<DraftAnswerItem>,
}

/// Stages emitted before `CHAT_FINAL` for a given early exit.
#[cfg(test)]
fn chat_stage_sequence(early_not_found: bool) -> Vec<&'static str> {
    if early_not_found {
        vec!["passages", "ranking"]
    } else {
        vec!["passages", "ranking", "writing", "checking"]
    }
}

/// Last up to `n` complete user+assistant pairs (assistant content used as-is).
pub fn reduce_history(history: &[ChatTurn], n_pairs: usize) -> Vec<ChatTurn> {
    let mut pairs: Vec<(ChatTurn, ChatTurn)> = Vec::new();
    let mut i = 0;
    while i + 1 < history.len() {
        let a = &history[i];
        let b = &history[i + 1];
        if a.role == "user" && b.role == "assistant" {
            pairs.push((a.clone(), b.clone()));
            i += 2;
        } else {
            i += 1;
        }
    }
    let start = pairs.len().saturating_sub(n_pairs);
    pairs[start..]
        .iter()
        .flat_map(|(u, a)| [u.clone(), a.clone()])
        .collect()
}

/// Parse model JSON and reject answerable drafts whose sentences lack quote indexes.
pub fn parse_grounded_answer(raw: &str) -> Result<GroundedDraft, String> {
    let trimmed = raw.trim();
    let draft = match serde_json::from_str::<GroundedDraft>(trimmed) {
        Ok(d) => d,
        Err(e) => {
            let extracted = trimmed
                .find('{')
                .and_then(|start| trimmed.rfind('}').map(|end| (start, end)))
                .filter(|(start, end)| end > start)
                .and_then(|(start, end)| serde_json::from_str(&trimmed[start..=end]).ok());
            extracted.ok_or_else(|| format!("grounded answer JSON: {e}"))?
        }
    };
    validate_draft(draft)
}

fn validate_draft(draft: GroundedDraft) -> Result<GroundedDraft, String> {
    if draft.answerable {
        for (i, item) in draft.answer.iter().enumerate() {
            if item.quotes.is_empty() {
                return Err(format!(
                    "answer sentence {i} has no quote indexes while answerable is true"
                ));
            }
        }
    }
    Ok(draft)
}

fn emit_stage(
    app: &AppHandle,
    message_id: &str,
    stage: &str,
    citations: Option<Vec<CitationItem>>,
    detail: Option<String>,
    draft: Vec<String>,
) {
    let _ = app.emit(
        CHAT_STAGE,
        &ChatStageEvent {
            message_id: message_id.to_string(),
            stage: stage.to_string(),
            citations,
            detail,
            draft,
        },
    );
}

async fn sidecar_request(
    state: &SharedState,
    method: &str,
    params: Value,
) -> CognitoResult<Value> {
    let state = state.clone();
    let method = method.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        state.sidecar.with_session(|s| s.request(&method, params))
    })
    .await
    .map_err(|e| crate::errors::CognitoError::Sidecar(e.to_string()))?
}

fn emit_final(app: &AppHandle, result: &ChatFinalResult) {
    let _ = app.emit(CHAT_FINAL, result);
}

fn parse_citations(prep: &Value) -> Vec<CitationItem> {
    prep.get("citations")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

fn build_generation_messages(
    query: &str,
    history: &[ChatTurn],
    citations: &[CitationItem],
) -> Vec<Value> {
    let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    for turn in reduce_history(history, 2) {
        messages.push(json!({ "role": turn.role, "content": turn.content }));
    }
    let mut passage_block = String::new();
    for c in citations {
        let idx = if c.index == 0 { 1 } else { c.index };
        let body = if c.text.is_empty() {
            c.snippet.as_str()
        } else {
            c.text.as_str()
        };
        passage_block.push_str(&format!("[{idx}] {}\n{body}\n\n", c.title));
    }
    let user_content = format!("Question: {query}\n\nPassages:\n{passage_block}");
    messages.push(json!({ "role": "user", "content": user_content }));
    messages
}

fn unanswerable_draft() -> GroundedDraft {
    GroundedDraft {
        answerable: false,
        quotes: vec![],
        answer: vec![],
    }
}

pub async fn run_chat_query(
    state: SharedState,
    app: AppHandle,
    query: String,
    message_id: String,
    history: Vec<ChatTurn>,
    scope_ids: Vec<String>,
) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let app_data = vault::path_string(&state.data_dir);
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    let hist_for_retrieval = reduce_history(&history, 2);

    let prep = match sidecar_request(
        &state,
        "prepare_chat_retrieval",
        json!({
            "sqlite_path": vault::path_string(&sqlite_path),
            "query": query,
            "embed_model": settings.embed_model,
            "history": hist_for_retrieval,
            "scope_ids": scope_ids,
            "app_data": app_data,
            "skip_rerank": true,
        }),
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            emit_final(
                &app,
                &ChatFinalResult::failure(message_id.clone(), e.to_string()),
            );
            return Ok(());
        }
    };

    let retrieval_ms = prep
        .get("retrievalMs")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let mut candidates = prep
        .get("candidates")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    if candidates.is_empty() {
        candidates = prep
            .get("citations")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
    }
    let preview: Vec<CitationItem> = candidates
        .iter()
        .take(5)
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    emit_stage(&app, &message_id, "passages", Some(preview), None, vec![]);

    let mut scope_chosen = prep
        .get("scopeChosen")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let mut scope_title = if scope_chosen {
        prep.get("scopeTitle")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };
    let mut result_scope_ids: Vec<String> = prep
        .get("scopeIds")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| scope_ids.clone());
    let query_embedding_failed = prep
        .get("queryEmbeddingFailed")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let profile_matches = prep
        .get("profileMatches")
        .cloned()
        .unwrap_or(json!([]));
    let labels = prep.get("labels").cloned().unwrap_or(json!([]));
    let normalised_question = prep
        .get("normalisedQuestion")
        .and_then(|x| x.as_str())
        .unwrap_or(query.as_str())
        .to_string();
    let scope_reason = prep
        .get("scopeReason")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let candidate_counts = prep
        .get("candidateCounts")
        .cloned()
        .unwrap_or(json!({}));
    let prep_trace = prep.get("trace").cloned();
    let explicit_scope = !scope_ids.is_empty();

    emit_stage(
        &app,
        &message_id,
        "ranking",
        None,
        Some("Ranking passages".into()),
        vec![],
    );

    let search_text = prep
        .get("searchText")
        .and_then(|x| x.as_str())
        .unwrap_or(query.as_str())
        .to_string();

    let ranked = if candidates.is_empty() {
        json!({
            "citations": [],
            "notFound": true,
            "rerankMs": 0,
            "rerankScored": 0,
            "suggestions": prep.get("suggestions").cloned().unwrap_or(json!([])),
            "trace": prep_trace.clone().unwrap_or(json!({})),
        })
    } else {
        match sidecar_request(
            &state,
            "rerank_chat_candidates",
            json!({
                "app_data": app_data,
                "query": search_text,
                "candidates": candidates,
                "sqlite_path": vault::path_string(&sqlite_path),
                "embed_model": settings.embed_model,
                "profile_matches": profile_matches,
                "explicit_scope": explicit_scope,
                "labels": labels,
                "normalised_question": normalised_question,
                "scope_ids": result_scope_ids,
                "scope_reason": scope_reason,
                "candidate_counts": candidate_counts,
            }),
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                emit_final(
                    &app,
                    &{
                    let mut r = ChatFinalResult::failure(message_id.clone(), e.to_string());
                    r.scope_ids = result_scope_ids;
                    r.timings = Some(ChatTimings {
                        retrieval_ms,
                        ..ChatTimings::default()
                    });
                    r
                },
                );
                return Ok(());
            }
        }
    };

    let citations = parse_citations(&ranked);
    let not_found = ranked
        .get("notFound")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let rerank_ms = ranked
        .get("rerankMs")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let rerank_scored = ranked
        .get("rerankScored")
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;

    if ranked
        .get("scopeChosen")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        scope_chosen = true;
        scope_title = ranked
            .get("scopeTitle")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        if let Some(arr) = ranked.get("scopeIds").and_then(|x| x.as_array()) {
            result_scope_ids = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
        }
    }
    let display_scope_title = if scope_chosen {
        scope_title.clone()
    } else {
        None
    };

    let suggestions: Vec<ScopeSuggestion> = ranked
        .get("suggestions")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    let trace = ranked
        .get("trace")
        .cloned()
        .or(prep_trace);

    let early = not_found || (query_embedding_failed && citations.is_empty());
    let early_mode = if query_embedding_failed && citations.is_empty() {
        "error"
    } else {
        "vector_only"
    };
    let timings = ChatTimings {
        retrieval_ms,
        rerank_ms,
        rerank_scored,
        generation_ms: 0,
        check_ms: 0,
    };
    finish_grounded_answer(
        state,
        app,
        query,
        message_id,
        history,
        citations,
        early,
        early_mode,
        "vector_only",
        display_scope_title,
        result_scope_ids,
        &settings.chat_model,
        &app_data,
        timings,
        suggestions,
        trace,
    )
    .await
}

pub async fn run_related_query(
    state: SharedState,
    app: AppHandle,
    entity_id_a: String,
    entity_id_b: String,
    message_id: String,
) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let app_data = vault::path_string(&state.data_dir);
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    let (name_a, name_b) = state.with_sqlite(|c| sqlite::entity_names(c, &entity_id_a, &entity_id_b))?;
    let query = format!("How are {name_a} and {name_b} related?");

    let prep = match sidecar_request(
        &state,
        "prepare_related_retrieval",
        json!({
            "sqlite_path": vault::path_string(&sqlite_path),
            "entity_id_a": entity_id_a,
            "entity_id_b": entity_id_b,
            "embed_model": settings.embed_model,
            "app_data": app_data,
        }),
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            emit_final(
                &app,
                &ChatFinalResult::failure(message_id.clone(), e.to_string()),
            );
            return Ok(());
        }
    };

    let citations = parse_citations(&prep);
    emit_stage(
        &app,
        &message_id,
        "passages",
        Some(citations.clone()),
        None,
        vec![],
    );
    emit_stage(
        &app,
        &message_id,
        "ranking",
        None,
        Some("Ranking passages".into()),
        vec![],
    );
    let empty = citations.is_empty();
    finish_grounded_answer(
        state,
        app,
        query,
        message_id,
        vec![],
        citations,
        empty,
        "related",
        "related",
        None,
        vec![],
        &settings.chat_model,
        &app_data,
        ChatTimings::default(),
        vec![],
        None,
    )
    .await
}

async fn finish_grounded_answer(
    state: SharedState,
    app: AppHandle,
    query: String,
    message_id: String,
    history: Vec<ChatTurn>,
    citations: Vec<CitationItem>,
    early_not_found: bool,
    early_mode: &str,
    success_mode: &str,
    scope_title: Option<String>,
    result_scope_ids: Vec<String>,
    chat_model: &str,
    app_data: &str,
    mut timings: ChatTimings,
    suggestions: Vec<ScopeSuggestion>,
    trace: Option<Value>,
) -> CognitoResult<()> {
    // Passages + ranking already emitted by the caller for chat; re-emit passages with final set.
    emit_stage(
        &app,
        &message_id,
        "passages",
        Some(citations.clone()),
        None,
        vec![],
    );

    if early_not_found {
        eprintln!(
            "chat timings retrieval={}ms rerank={}ms scored={} generation=0ms check=0ms",
            timings.retrieval_ms, timings.rerank_ms, timings.rerank_scored
        );
        emit_final(
            &app,
            &ChatFinalResult {
                message_id,
                citations,
                retrieval_mode: early_mode.into(),
                error: None,
                not_found: true,
                sentences: vec![],
                scope_title,
                scope_ids: result_scope_ids,
                timings: Some(timings),
                suggestions,
                trace,
            },
        );
        return Ok(());
    }

    emit_stage(
        &app,
        &message_id,
        "writing",
        None,
        Some("Quoting the sources".into()),
        vec![],
    );

    let messages = build_generation_messages(&query, &history, &citations);
    let gen_started = Instant::now();
    let app_write = app.clone();
    let mid_write = message_id.clone();
    let mut emitted_draft_len = 0usize;
    let raw = match ollama_stream::chat_completion_json_streaming(
        chat_model,
        messages,
        answer_format_schema(),
        |acc| {
            let draft_now = ollama_stream::draft_sentences_from_partial(acc);
            if draft_now.len() > emitted_draft_len {
                emitted_draft_len = draft_now.len();
                emit_stage(&app_write, &mid_write, "writing", None, None, draft_now);
            }
        },
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            emit_final(
                &app,
                &{
                    let mut r = ChatFinalResult::failure(message_id, e);
                    r.citations = citations;
                    r.scope_title = scope_title;
                    r.scope_ids = result_scope_ids;
                    r.timings = Some(timings);
                    r
                },
            );
            return Ok(());
        }
    };
    timings.generation_ms = gen_started.elapsed().as_millis() as u64;

    let draft = match parse_grounded_answer(&raw) {
        Ok(d) => d,
        Err(e) => {
            emit_final(
                &app,
                &{
                    let mut r = ChatFinalResult::failure(message_id, e);
                    r.citations = citations;
                    r.scope_title = scope_title;
                    r.scope_ids = result_scope_ids;
                    r.timings = Some(timings);
                    r
                },
            );
            return Ok(());
        }
    };

    let draft = if draft.answerable && draft.answer.is_empty() {
        unanswerable_draft()
    } else {
        draft
    };

    let n_sentences = draft.answer.len();
    let writing_draft: Vec<String> = draft.answer.iter().map(|a| a.sentence.clone()).collect();
    emit_stage(
        &app,
        &message_id,
        "checking",
        None,
        Some(format!("Checking {n_sentences} sentences")),
        writing_draft,
    );

    let passages: Vec<Value> = citations
        .iter()
        .map(|c| {
            let idx = if c.index == 0 { 1 } else { c.index };
            let body = if c.text.is_empty() {
                c.snippet.as_str()
            } else {
                c.text.as_str()
            };
            json!({ "index": idx, "text": body })
        })
        .collect();

    let check_started = Instant::now();
    let check = match sidecar_request(
        &state,
        "check_answer",
        json!({
            "app_data": app_data,
            "passages": passages,
            "draft": draft,
        }),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            emit_final(
                &app,
                &{
                    let mut r = ChatFinalResult::failure(message_id, e.to_string());
                    r.citations = citations;
                    r.scope_title = scope_title;
                    r.scope_ids = result_scope_ids;
                    r.timings = Some(timings);
                    r
                },
            );
            return Ok(());
        }
    };
    timings.check_ms = check_started.elapsed().as_millis() as u64;

    let sentences: Vec<AnswerSentence> = check
        .get("sentences")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    let check_not_found = check
        .get("notFound")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    eprintln!(
        "chat timings retrieval={}ms rerank={}ms scored={} generation={}ms check={}ms",
        timings.retrieval_ms,
        timings.rerank_ms,
        timings.rerank_scored,
        timings.generation_ms,
        timings.check_ms
    );

    emit_final(
        &app,
        &ChatFinalResult {
            message_id,
            citations,
            retrieval_mode: success_mode.into(),
            error: None,
            not_found: check_not_found,
            sentences,
            scope_title,
            scope_ids: result_scope_ids,
            timings: Some(timings),
            suggestions,
            trace,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_answerable_sentence_without_quotes() {
        let raw = r#"{
            "answerable": true,
            "quotes": [{"source": 1, "text": "hello"}],
            "answer": [{"sentence": "Hello.", "quotes": []}]
        }"#;
        let err = parse_grounded_answer(raw).unwrap_err();
        assert!(err.contains("no quote indexes"), "{err}");
    }

    #[test]
    fn parse_accepts_answerable_with_quotes() {
        let raw = r#"{
            "answerable": true,
            "quotes": [{"source": 1, "text": "hello"}],
            "answer": [{"sentence": "Hello.", "quotes": [0]}]
        }"#;
        let d = parse_grounded_answer(raw).unwrap();
        assert!(d.answerable);
        assert_eq!(d.answer[0].quotes, vec![0]);
    }

    #[test]
    fn parse_allows_unanswerable_empty() {
        let raw = r#"{"answerable": false, "quotes": [], "answer": []}"#;
        let d = parse_grounded_answer(raw).unwrap();
        assert!(!d.answerable);
        assert!(d.answer.is_empty());
    }

    #[test]
    fn stage_sequence_early_not_found() {
        assert_eq!(chat_stage_sequence(true), vec!["passages", "ranking"]);
        assert_eq!(
            chat_stage_sequence(false),
            vec!["passages", "ranking", "writing", "checking"]
        );
    }

    #[test]
    fn reduce_history_keeps_last_two_pairs() {
        let hist = vec![
            ChatTurn {
                role: "user".into(),
                content: "q1".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "a1".into(),
            },
            ChatTurn {
                role: "user".into(),
                content: "q2".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "a2".into(),
            },
            ChatTurn {
                role: "user".into(),
                content: "q3".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "a3".into(),
            },
        ];
        let reduced = reduce_history(&hist, 2);
        assert_eq!(reduced.len(), 4);
        assert_eq!(reduced[0].content, "q2");
        assert_eq!(reduced[3].content, "a3");
    }
}
