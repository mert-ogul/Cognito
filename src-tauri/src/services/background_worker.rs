//! Background work: entity extraction, then document cards, then link recompute.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::events::{BACKGROUND_PROGRESS, LIBRARY_CHANGED};
use crate::models::BackgroundProgress;
use crate::services::chat_gate::{self, WaitKind};
use crate::services::document_cards;
use crate::services::ollama_stream;
use crate::vault;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub(crate) fn emit_progress(
    app: Option<&AppHandle>,
    job_id: Option<&str>,
    document_id: &str,
    phase: &str,
    detail: Option<&str>,
) {
    let Some(a) = app else {
        return;
    };
    let _ = a.emit(
        BACKGROUND_PROGRESS,
        &BackgroundProgress {
            job_id: job_id.map(String::from),
            document_id: document_id.to_string(),
            phase: phase.to_string(),
            detail: detail.map(String::from),
        },
    );
}

fn background_and_chat_models(state: &SharedState) -> (String, String) {
    state
        .with_sqlite(sqlite::load_settings)
        .map(|s| (s.background_model, s.chat_model))
        .unwrap_or_else(|_| ("qwen3.5:9b".into(), "qwen3.5:9b".into()))
}

fn unload_background(state: &SharedState) {
    let (background, chat) = background_and_chat_models(state);
    if background == chat {
        return;
    }
    ollama_stream::unload_model_blocking(&background);
}

enum DocOutcome {
    Finished,
    Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessPendingOutcome {
    pub finished: usize,
    pub paused: bool,
}

/// Test hook: increments on each `process_pending` entry.
pub static PROCESS_PENDING_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn process_one_entities(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<DocOutcome> {
    let settings = state.with_sqlite(sqlite::load_settings)?;
    if !settings.extract_entities {
        return Ok(DocOutcome::Finished);
    }
    state.with_sqlite(|c| {
        sqlite::mark_document_entities_running(c, document_id)?;
        sqlite::clear_document_entity_rows(c, document_id)?;
        Ok(())
    })?;
    emit_progress(app, None, document_id, "entities", None);
    if !chat_gate::wait_while_chat_in_flight(state, document_id, WaitKind::Entities)? {
        return Ok(DocOutcome::Finished);
    }
    let sqlite_path = state.sqlite_path();
    let app_data = vault::path_string(&state.data_dir);
    let labels: Vec<String> = settings
        .extra_entity_labels
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let mut chunk_offset = 0i64;
    loop {
        if !chat_gate::wait_while_chat_in_flight(state, document_id, WaitKind::Entities)? {
            return Ok(DocOutcome::Finished);
        }
        let res = state.sidecar.with_session(|s| {
            s.request(
                "extract_entities",
                json!({
                    "sqlitePath": vault::path_string(&sqlite_path),
                    "documentId": document_id,
                    "app_data": app_data,
                    "labels": labels,
                    "chunkOffset": chunk_offset,
                    "chunkLimit": 40,
                }),
            )
        });
        match res {
            Ok(v) => {
                let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                if !ok {
                    let msg = v
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("entity extraction failed")
                        .to_string();
                    state.with_sqlite(|c| {
                        sqlite::mark_document_entities_failed(c, document_id, &msg)
                    })?;
                    emit_progress(app, None, document_id, "failed", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    return Ok(DocOutcome::Finished);
                }
                let total = v
                    .get("totalChunks")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0)
                    .max(0) as usize;
                let next = v
                    .get("nextOffset")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(chunk_offset);
                let done = v.get("done").and_then(|x| x.as_bool()).unwrap_or(false);
                let shown = next.max(0) as usize;
                let detail = format!("{shown} of {total} passages");
                emit_progress(app, None, document_id, "entities", Some(&detail));
                if done {
                    state
                        .with_sqlite(|c| sqlite::mark_document_entities_ok(c, document_id, None))?;
                    emit_progress(app, None, document_id, "ok", None);
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    return Ok(DocOutcome::Finished);
                }
                chunk_offset = next;
            }
            Err(e) => {
                let msg = e.to_string();
                if crate::services::checking_models::is_missing_model_error(&msg) {
                    state
                        .with_sqlite(|c| sqlite::mark_document_entities_pending(c, document_id))?;
                    emit_progress(app, None, document_id, "pending", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    return Ok(DocOutcome::Pause);
                }
                if ollama_stream::ollama_unreachable(&e) {
                    state
                        .with_sqlite(|c| sqlite::mark_document_entities_pending(c, document_id))?;
                    emit_progress(app, None, document_id, "pending", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    return Ok(DocOutcome::Pause);
                }
                state
                    .with_sqlite(|c| sqlite::mark_document_entities_failed(c, document_id, &msg))?;
                emit_progress(app, None, document_id, "failed", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
                return Ok(DocOutcome::Finished);
            }
        }
    }
}

fn process_one_card(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<DocOutcome> {
    let settings = state.with_sqlite(sqlite::load_settings)?;
    if !settings.summarise_documents {
        return Ok(DocOutcome::Finished);
    }
    state.with_sqlite(|c| sqlite::mark_document_card_running(c, document_id))?;
    emit_progress(app, None, document_id, "summarising", None);
    if !chat_gate::wait_while_chat_in_flight(state, document_id, WaitKind::Card)? {
        return Ok(DocOutcome::Finished);
    }
    match document_cards::build_and_store_card(state, document_id) {
        Ok(()) => {
            state.with_sqlite(|c| sqlite::mark_document_card_ok(c, document_id))?;
            emit_progress(app, None, document_id, "ok", None);
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            Ok(DocOutcome::Finished)
        }
        Err(crate::errors::CognitoError::InterruptedByChat) => {
            eprintln!("card paused for a question: {document_id}");
            state.with_sqlite(|c| sqlite::mark_document_card_pending(c, document_id))?;
            emit_progress(app, None, document_id, "pending", None);
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            let _ = chat_gate::wait_while_chat_in_flight(state, document_id, WaitKind::Card)?;
            Ok(DocOutcome::Finished)
        }
        Err(e) => {
            let msg = e.to_string();
            if ollama_stream::ollama_unreachable(&e) {
                state.with_sqlite(|c| sqlite::mark_document_card_pending(c, document_id))?;
                emit_progress(app, None, document_id, "pending", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
                return Ok(DocOutcome::Pause);
            }
            state.with_sqlite(|c| sqlite::mark_document_card_failed(c, document_id, &msg))?;
            emit_progress(app, None, document_id, "failed", Some(&msg));
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            Ok(DocOutcome::Finished)
        }
    }
}

fn recompute_links(state: &SharedState) {
    let sqlite_path = state.sqlite_path();
    let _ = state.sidecar.with_session(|s| {
        s.request(
            "recompute_links",
            json!({ "sqlitePath": vault::path_string(&sqlite_path) }),
        )
    });
}

/// Processes pending entities, then cards, then recomputes links when idle.
pub fn process_pending(state: &SharedState, app: Option<&AppHandle>) -> ProcessPendingOutcome {
    PROCESS_PENDING_CALLS.fetch_add(1, Ordering::SeqCst);
    let mut finished = 0usize;
    let mut pause = false;

    loop {
        let enabled = state
            .with_sqlite(sqlite::load_settings)
            .map(|s| s.extract_entities)
            .unwrap_or(true);
        if !enabled {
            break;
        }
        let next = state
            .with_sqlite(sqlite::next_pending_entities_document)
            .ok()
            .flatten();
        let Some(doc_id) = next else {
            break;
        };
        match process_one_entities(state, app, &doc_id) {
            Ok(DocOutcome::Finished) => {}
            Ok(DocOutcome::Pause) => {
                pause = true;
                break;
            }
            Err(e) => {
                let msg = e.to_string();
                if ollama_stream::ollama_unreachable(&e) {
                    let _ =
                        state.with_sqlite(|c| sqlite::mark_document_entities_pending(c, &doc_id));
                    emit_progress(app, None, &doc_id, "pending", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    pause = true;
                    break;
                }
                let _ =
                    state.with_sqlite(|c| sqlite::mark_document_entities_failed(c, &doc_id, &msg));
                emit_progress(app, None, &doc_id, "failed", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
            }
        }
        finished += 1;
    }

    let entity_queue_empty = state
        .with_sqlite(sqlite::next_pending_entities_document)
        .ok()
        .flatten()
        .is_none();
    if entity_queue_empty {
        let _ = state
            .sidecar
            .with_session(|s| s.request("unload_entity_model", json!({})));
    }

    if !pause {
        loop {
            let summarise = state
                .with_sqlite(sqlite::load_settings)
                .map(|s| s.summarise_documents)
                .unwrap_or(true);
            if !summarise {
                break;
            }
            let next = state
                .with_sqlite(sqlite::next_pending_card_document)
                .ok()
                .flatten();
            let Some(doc_id) = next else {
                break;
            };
            match process_one_card(state, app, &doc_id) {
                Ok(DocOutcome::Finished) => {}
                Ok(DocOutcome::Pause) => {
                    pause = true;
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let _ =
                        state.with_sqlite(|c| sqlite::mark_document_card_failed(c, &doc_id, &msg));
                    emit_progress(app, None, &doc_id, "failed", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                }
            }
            finished += 1;
        }
    }

    if !pause {
        let busy = state
            .with_sqlite(sqlite::pending_entities_or_cards_remain)
            .unwrap_or(true);
        if !busy {
            recompute_links(state);
        }
        unload_background(state);
    }
    ProcessPendingOutcome {
        finished,
        paused: pause,
    }
}

pub fn spawn(state: SharedState, app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    {
        let mut g = state.background_tx.lock().expect("background_tx lock");
        *g = Some(tx);
    }
    let _ = state.with_sqlite(|c| {
        sqlite::reset_running_card_jobs(c)?;
        sqlite::reset_running_entities_jobs(c)?;
        Ok(())
    });
    thread::spawn(move || worker_loop(state, app, rx));
}

fn drain_pending(state: &SharedState, app: Option<&AppHandle>) {
    loop {
        let outcome = process_pending(state, app);
        if outcome.paused || outcome.finished == 0 {
            break;
        }
    }
}

fn worker_loop(state: SharedState, app: AppHandle, rx: Receiver<()>) {
    while let Ok(()) | Err(RecvTimeoutError::Timeout) = rx.recv_timeout(Duration::from_secs(30)) {
        drain_pending(&state, Some(&app));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::sidecar::SidecarPool;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering as AtomicOrdering;
    use std::sync::{Arc, Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cards_deleted_on_reindex() {
        let data = tempfile::tempdir().expect("data tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
            data.path().to_path_buf(),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        let doc_id = "doc-reindex-card";
        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c,
                    doc_id,
                    "/tmp/x.md",
                    "X",
                    "markdown",
                    "hash1",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                sqlite::upsert_document_card(
                    c,
                    doc_id,
                    "note",
                    "X",
                    None,
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "qwen3.5:9b",
                )?;
                let n: i64 = c.query_row(
                    "SELECT COUNT(*) FROM document_cards WHERE document_id = ?1",
                    [doc_id],
                    |r| r.get(0),
                )?;
                assert_eq!(n, 1);
                sqlite::delete_document_by_id(c, doc_id)?;
                let n2: i64 = c.query_row(
                    "SELECT COUNT(*) FROM document_cards WHERE document_id = ?1",
                    [doc_id],
                    |r| r.get(0),
                )?;
                assert_eq!(n2, 0);
                Ok(())
            })
            .expect("cascade");
    }

    #[test]
    fn process_pending_entities_before_cards() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("COGNITO_MOCK_MODELS", "1");
        let data = tempfile::tempdir().expect("data tempdir");
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let pool = SidecarPool::new(
            main_py,
            sidecar_cwd,
            data.path().join("sidecar.log"),
            data.path().to_path_buf(),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state.sidecar.ensure().expect("sidecar");
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "extract_entities", "true")?;
                sqlite::upsert_setting(c, "summarise_documents", "false")?;
                sqlite::insert_document_row(
                    c,
                    "doc-ent",
                    "/tmp/e.md",
                    "E",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                sqlite::insert_chunk_row(
                    c,
                    "c-ent",
                    "doc-ent",
                    0,
                    None,
                    None,
                    "Ada Chen works at Meridian Labs in Cambridge.",
                    8,
                    "h",
                    "body",
                    "[]",
                    None, None)?;
                c.execute(
                    "UPDATE documents SET entities_status = 'pending', card_status = 'pending' WHERE id = 'doc-ent'",
                    [],
                )?;
                Ok(())
            })
            .expect("seed");

        let outcome = process_pending(&state, None);
        assert_eq!(outcome.finished, 1);
        assert!(!outcome.paused);
        let (status, err): (String, Option<String>) = state
            .with_sqlite(|c| {
                Ok(c.query_row(
                    "SELECT entities_status, entities_error FROM documents WHERE id = 'doc-ent'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .expect("status");
        assert_eq!(status, "ok", "entities_error={err:?}");
        std::env::remove_var("COGNITO_MOCK_MODELS");
    }

    #[test]
    fn background_model_unloaded_when_chat_starts() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("COGNITO_MOCK_LLM", "1");
        ollama_stream::UNLOAD_CALLS.store(0, AtomicOrdering::SeqCst);
        let data = tempfile::tempdir().expect("data tempdir");
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
                sqlite::upsert_setting(c, "extract_entities", "false")?;
                sqlite::upsert_setting(c, "chat_model", "qwen3.5:9b")?;
                sqlite::upsert_setting(c, "background_model", "qwen3.5:4b")?;
                sqlite::insert_document_row(
                    c,
                    "doc-unload",
                    "/tmp/u.md",
                    "U",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                c.execute(
                    "UPDATE documents SET card_status = 'running' WHERE id = 'doc-unload'",
                    [],
                )?;
                Ok(())
            })
            .expect("seed");
        state.chat_in_flight.fetch_add(1, AtomicOrdering::SeqCst);
        let before = ollama_stream::UNLOAD_CALLS.load(AtomicOrdering::SeqCst);
        let handle = {
            let state = state.clone();
            thread::spawn(move || {
                chat_gate::wait_while_chat_in_flight(&state, "doc-unload", WaitKind::Card)
            })
        };
        thread::sleep(Duration::from_millis(600));
        let mid = ollama_stream::UNLOAD_CALLS.load(AtomicOrdering::SeqCst);
        assert!(mid > before, "expected unload while chat in flight");
        state.chat_in_flight.fetch_sub(1, AtomicOrdering::SeqCst);
        let ok = handle.join().expect("join").expect("wait");
        assert!(ok);
        std::env::remove_var("COGNITO_MOCK_LLM");
    }

    #[test]
    fn unload_background_noop_when_models_match() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("COGNITO_MOCK_LLM", "1");
        ollama_stream::UNLOAD_CALLS.store(0, AtomicOrdering::SeqCst);
        let data = tempfile::tempdir().expect("data tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
            data.path().to_path_buf(),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "chat_model", "qwen3.5:9b")?;
                sqlite::upsert_setting(c, "background_model", "qwen3.5:9b")?;
                Ok(())
            })
            .expect("settings");
        let before = ollama_stream::UNLOAD_CALLS.load(AtomicOrdering::SeqCst);
        unload_background(&state);
        assert_eq!(
            ollama_stream::UNLOAD_CALLS.load(AtomicOrdering::SeqCst),
            before
        );
        std::env::remove_var("COGNITO_MOCK_LLM");
    }

    #[test]
    fn worker_loop_does_not_spin_when_ollama_unreachable() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::remove_var("COGNITO_MOCK_LLM");
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:9");
        PROCESS_PENDING_CALLS.store(0, AtomicOrdering::SeqCst);
        let data = tempfile::tempdir().expect("data tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
            data.path().to_path_buf(),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "extract_entities", "false")?;
                sqlite::upsert_setting(c, "summarise_documents", "true")?;
                sqlite::insert_document_row(
                    c,
                    "doc-pause",
                    "/tmp/p.md",
                    "P",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                sqlite::insert_chunk_row(
                    c,
                    "c-pause",
                    "doc-pause",
                    0,
                    None,
                    None,
                    "Some body text for the card.",
                    5,
                    "h",
                    "body",
                    "[]",
                    None,
                    None,
                )?;
                c.execute(
                    "UPDATE documents SET card_status = 'pending' WHERE id = 'doc-pause'",
                    [],
                )?;
                Ok(())
            })
            .expect("seed");

        let (tx, rx) = mpsc::channel::<()>();
        let state_w = state.clone();
        let handle = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {
                        drain_pending(&state_w, None);
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        let _ = tx.send(());
        handle.join().expect("worker");
        std::env::remove_var("OLLAMA_HOST");
        let calls = PROCESS_PENDING_CALLS.load(AtomicOrdering::SeqCst);
        assert!(
            calls > 0 && calls < 30,
            "expected a few process_pending calls over 2s, got {calls}"
        );
    }
}
