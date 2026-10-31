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

const SYSTEM_PROMPT: &str = "Answer only from the numbered passages. For each answer sentence, first copy one to three short, exact passages it relies on, with their passage numbers, then write the sentence from them. Don't repeat a fact in two sentences. Write every sentence in words. Use LaTeX only for real formulas, never for plain numbers, percentages or units: write 81.5%, not $81.5\\%$. Inside JSON strings, write each LaTeX backslash twice. Write formulas in LaTeX between $ or $$. Formulas in the passages come from PDF text and are often garbled; rewrite them as clean LaTeX. For example, h = σ(W x̃ + b) becomes $h = \\sigma(W\\tilde{x} + b)$. Each passage starts with its document's title. If the question asks about a specific paper, person or thing and the passages are about a different one, set answerable to false. If the passages don't answer the question, set answerable to false and leave the answer list empty.";

fn answer_format_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "answerable": { "type": "boolean" },
            "answer": {
                "type": "array",
                "maxItems": 5,
                "items": {
                    "type": "object",
                    "properties": {
                        "quotes": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 3,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "source": { "type": "integer" },
                                    "text": { "type": "string" }
                                },
                                "required": ["source", "text"]
                            }
                        },
                        "sentence": { "type": "string" }
                    },
                    "required": ["quotes", "sentence"]
                }
            }
        },
        "required": ["answerable", "answer"]
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
    pub quotes: Vec<DraftQuote>,
    pub sentence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedDraft {
    pub answerable: bool,
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

/// Undo JSON control-character decoding that broke LaTeX (`\t` → tab, etc.).
pub fn repair_latex_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\t' => out.push_str("\\t"),
            '\u{000c}' => out.push_str("\\f"),
            '\u{0008}' => out.push_str("\\b"),
            '\r' => out.push_str("\\r"),
            '\n' => {
                if chars.peek().is_some_and(|n| n.is_ascii_alphabetic()) {
                    out.push_str("\\n");
                } else {
                    out.push(' ');
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn sentence_word_key(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn dedupe_answer_sentences(mut draft: GroundedDraft) -> GroundedDraft {
    let mut seen = std::collections::HashSet::new();
    draft
        .answer
        .retain(|item| seen.insert(sentence_word_key(&item.sentence)));
    draft
}

/// Parse model JSON; answerable drafts with no quoted sentences become unanswerable.
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
    Ok(validate_draft(dedupe_answer_sentences(repair_draft_sentences(draft))))
}

fn repair_draft_sentences(mut draft: GroundedDraft) -> GroundedDraft {
    for item in &mut draft.answer {
        item.sentence = repair_latex_escapes(&item.sentence);
    }
    draft
}

fn validate_draft(mut draft: GroundedDraft) -> GroundedDraft {
    if draft.answerable {
        let any_quoted = draft.answer.iter().any(|item| !item.quotes.is_empty());
        if !any_quoted {
            draft.answerable = false;
            draft.answer.clear();
        }
    }
    draft
}

fn emit_stage(
    app: &AppHandle,
    message_id: &str,
    conversation_id: &str,
    stage: &str,
    citations: Option<Vec<CitationItem>>,
    detail: Option<String>,
    draft: Vec<String>,
) {
    let _ = app.emit(
        CHAT_STAGE,
        &ChatStageEvent {
            message_id: message_id.to_string(),
            conversation_id: conversation_id.to_string(),
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

fn assistant_content_and_status(result: &ChatFinalResult) -> (String, &'static str) {
    if let Some(err) = &result.error {
        return (err.clone(), "error");
    }
    let content = result
        .sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    (content, "done")
}

fn assistant_payload(result: &ChatFinalResult) -> Value {
    json!({
        "citations": result.citations,
        "sentences": result.sentences,
        "notFound": result.not_found,
        "scopeTitle": result.scope_title,
        "suggestions": result.suggestions,
        "error": result.error,
    })
}

/// Persist the assistant row, then emit. A save failure is logged; the event still fires.
pub fn emit_final(state: &SharedState, app: &AppHandle, result: &ChatFinalResult) {
    let (content, status) = assistant_content_and_status(result);
    let payload = assistant_payload(result);
    if let Err(e) = state.with_sqlite(|c| {
        sqlite::conversations::finish_assistant_message(
            c,
            &result.message_id,
            status,
            &content,
            Some(&payload),
        )
    }) {
        eprintln!("chat save failed for {}: {e}", result.message_id);
    }
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
        answer: vec![],
    }
}

pub async fn run_chat_query(
    state: SharedState,
    app: AppHandle,
    query: String,
    message_id: String,
    conversation_id: String,
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
                &state,
                &app,
                &ChatFinalResult::failure(
                    message_id.clone(),
                    conversation_id.clone(),
                    e.to_string(),
                ),
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
    emit_stage(
        &app,
        &message_id,
        &conversation_id,
        "passages",
        Some(preview),
        None,
        vec![],
    );

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
        &conversation_id,
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
                    &state,
                    &app,
                    &{
                        let mut r = ChatFinalResult::failure(
                            message_id.clone(),
                            conversation_id.clone(),
                            e.to_string(),
                        );
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
        conversation_id,
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
    conversation_id: String,
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
                &state,
                &app,
                &ChatFinalResult::failure(
                    message_id.clone(),
                    conversation_id.clone(),
                    e.to_string(),
                ),
            );
            return Ok(());
        }
    };

    let citations = parse_citations(&prep);
    emit_stage(
        &app,
        &message_id,
        &conversation_id,
        "passages",
        Some(citations.clone()),
        None,
        vec![],
    );
    emit_stage(
        &app,
        &message_id,
        &conversation_id,
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
        conversation_id,
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
    conversation_id: String,
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
        &conversation_id,
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
            &state,
            &app,
            &ChatFinalResult {
                message_id,
                conversation_id,
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
        &conversation_id,
        "writing",
        None,
        Some("Quoting the sources".into()),
        vec![],
    );

    let messages = build_generation_messages(&query, &history, &citations);
    let gen_started = Instant::now();
    let app_write = app.clone();
    let mid_write = message_id.clone();
    let cid_write = conversation_id.clone();
    let mut emitted_draft_len = 0usize;
    let raw = match ollama_stream::chat_completion_json_streaming(
        chat_model,
        messages,
        answer_format_schema(),
        |acc| {
            let draft_now: Vec<String> = ollama_stream::draft_sentences_from_partial(acc)
                .into_iter()
                .map(|s| repair_latex_escapes(&s))
                .collect();
            if draft_now.len() > emitted_draft_len {
                emitted_draft_len = draft_now.len();
                emit_stage(
                    &app_write,
                    &mid_write,
                    &cid_write,
                    "writing",
                    None,
                    None,
                    draft_now,
                );
            }
        },
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            emit_final(
                &state,
                &app,
                &{
                    let mut r = ChatFinalResult::failure(message_id, conversation_id, e);
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
                &state,
                &app,
                &{
                    let mut r = ChatFinalResult::failure(message_id, conversation_id, e);
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
        &conversation_id,
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
            json!({ "index": idx, "title": c.title, "text": body })
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
                &state,
                &app,
                &{
                    let mut r =
                        ChatFinalResult::failure(message_id, conversation_id, e.to_string());
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
    #[cfg(debug_assertions)]
    {
        eprintln!("chat draft json: {raw}");
        for (i, item) in draft.answer.iter().enumerate() {
            let status = sentences
                .get(i)
                .map(|s| s.status.as_str())
                .unwrap_or("");
            let reason = sentences
                .get(i)
                .and_then(|s| s.reason.as_deref())
                .unwrap_or("");
            eprintln!(
                "chat sentence {i} status={status} reason={reason} quotes={}",
                item.quotes.len()
            );
        }
    }

    emit_final(
        &state,
        &app,
        &ChatFinalResult {
            message_id,
            conversation_id,
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
    fn repair_latex_escapes_restores_commands() {
        assert_eq!(
            repair_latex_escapes(&format!("$81.5{}ext{{{}extperthousand}}$", '\t', '\t')),
            r"$81.5\text{\textperthousand}$"
        );
        assert_eq!(
            repair_latex_escapes(&format!("${}rac{{1}}{{2}}$", '\u{000c}')),
            r"$\frac{1}{2}$"
        );
        assert_eq!(
            repair_latex_escapes(&format!("${}eta$", '\u{0008}')),
            r"$\beta$"
        );
        assert_eq!(
            repair_latex_escapes(&format!("${}abla$", '\n')),
            r"$\nabla$"
        );
    }

    #[test]
    fn parse_dedupes_identical_sentences() {
        let raw = r#"{
            "answerable": true,
            "answer": [
                {"quotes": [{"source": 1, "text": "a"}], "sentence": "GCN reaches 81.5% on Cora."},
                {"quotes": [{"source": 2, "text": "b"}], "sentence": "GCN reaches 81.5% on Cora!"}
            ]
        }"#;
        let d = parse_grounded_answer(raw).unwrap();
        assert_eq!(d.answer.len(), 1);
        assert_eq!(d.answer[0].sentence, "GCN reaches 81.5% on Cora.");
    }

    #[test]
    fn parse_marks_unanswerable_when_no_sentence_has_quotes() {
        let raw = r#"{
            "answerable": true,
            "answer": [{"quotes": [], "sentence": "Hello."}]
        }"#;
        let d = parse_grounded_answer(raw).unwrap();
        assert!(!d.answerable);
        assert!(d.answer.is_empty());
    }

    #[test]
    fn parse_keeps_answerable_when_some_sentences_lack_quotes() {
        let raw = r#"{
            "answerable": true,
            "answer": [
                {"quotes": [{"source": 1, "text": "hello"}], "sentence": "Hello."},
                {"quotes": [], "sentence": "Extra."}
            ]
        }"#;
        let d = parse_grounded_answer(raw).unwrap();
        assert!(d.answerable);
        assert_eq!(d.answer.len(), 2);
        assert!(d.answer[1].quotes.is_empty());
    }

    #[test]
    fn parse_accepts_answerable_with_quotes() {
        let raw = r#"{
            "answerable": true,
            "answer": [{
                "quotes": [{"source": 1, "text": "hello"}],
                "sentence": "Hello."
            }]
        }"#;
        let d = parse_grounded_answer(raw).unwrap();
        assert!(d.answerable);
        assert_eq!(d.answer[0].quotes.len(), 1);
        assert_eq!(d.answer[0].quotes[0].source, 1);
        assert_eq!(d.answer[0].quotes[0].text, "hello");
    }

    #[test]
    fn parse_allows_unanswerable_empty() {
        let raw = r#"{"answerable": false, "answer": []}"#;
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
