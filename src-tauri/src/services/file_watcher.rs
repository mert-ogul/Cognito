//! Recursive vault watcher (notify + debounce) → incremental ingest / removal.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::events::LIBRARY_CHANGED;
use crate::services::ingest;
use crate::services::watcher_action;
use crate::vault;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter};


fn load_context(state: &SharedState) -> CognitoResult<(PathBuf, String, String)> {
    state.with_sqlite(|c| {
        let v = sqlite::get_setting(c, "vault_path")?
            .ok_or_else(|| CognitoError::Vault("no vault".into()))?;
        let s = sqlite::load_settings(c)?;
        Ok((PathBuf::from(v), s.embed_model, s.chat_model))
    })
}

fn purge_then_remove(state: &SharedState, path: &Path) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let id = state.with_sqlite(|c| sqlite::document_id_by_path(c, &vault::path_string(path)))?;
    let Some(doc_id) = id else {
        return Ok(());
    };
    let cids = state.with_sqlite(|c| sqlite::list_chunk_ids_for_document(c, &doc_id))?;
    state.with_sqlite(|c| sqlite::clear_document_entity_rows(c, &doc_id))?;
    if !cids.is_empty() {
        state.sidecar.with_session(|s| {
            s.request(
                "purge_chunk_embeddings",
                json!({ "sqlitePath": vault::path_string(&sqlite_path), "chunkIds": cids }),
            )?;
            Ok(())
        })?;
    }
    state.with_sqlite(|c| sqlite::mark_document_removed(c, &vault::path_string(path)))?;
    Ok(())
}

pub fn handle_watch_paths(state: &SharedState, app: Option<&AppHandle>, paths: &[PathBuf]) {
    let Ok((vault, embed, chat)) = load_context(state) else {
        return;
    };
    for p in paths {
        if !vault::is_importable_extension(p) {
            continue;
        }
        match watcher_action::action_for_disk_state(p.exists()) {
            "remove" => {
                let _ = purge_then_remove(state, p);
            }
            _ => {
                let _ = ingest::run_single_file(state, None, &vault, p, &embed, &chat);
            }
        }
    }
    if let Some(a) = app {
        let _ = a.emit(LIBRARY_CHANGED, &());
    }
}

fn watch_thread(root: PathBuf, stop: Arc<AtomicBool>, out: Sender<Vec<PathBuf>>) {
    use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebounceEventResult};
    let tx = out.clone();
    let debouncer = new_debouncer(Duration::from_millis(800), move |res: DebounceEventResult| {
        if let Ok(events) = res {
            let paths: Vec<PathBuf> = events.iter().map(|e| e.path.clone()).collect();
            if !paths.is_empty() {
                let _ = tx.send(paths);
            }
        }
    });
    let Ok(mut debouncer) = debouncer else {
        return;
    };
    let _ = debouncer
        .watcher()
        .watch(&root, RecursiveMode::Recursive);
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(200));
    }
}

fn restart_watcher(
    active: &mut Option<(Arc<AtomicBool>, JoinHandle<()>)>,
    root: PathBuf,
    file_tx: Sender<Vec<PathBuf>>,
) {
    if let Some((stop, j)) = active.take() {
        stop.store(true, Ordering::SeqCst);
        let _ = j.join();
    }
    let stop = Arc::new(AtomicBool::new(false));
    let st = stop.clone();
    let tx = file_tx.clone();
    let j = thread::spawn(move || watch_thread(root, st, tx));
    *active = Some((stop, j));
}

fn stop_active(active: &mut Option<(Arc<AtomicBool>, JoinHandle<()>)>) {
    if let Some((stop, j)) = active.take() {
        stop.store(true, Ordering::SeqCst);
        let _ = j.join();
    }
}

fn coordinator_loop(
    state: SharedState,
    app: AppHandle,
    reload_rx: Receiver<Option<PathBuf>>,
    file_rx: Receiver<Vec<PathBuf>>,
    file_tx: Sender<Vec<PathBuf>>,
) {
    let mut active: Option<(Arc<AtomicBool>, JoinHandle<()>)> = None;
    loop {
        while let Ok(reload) = reload_rx.try_recv() {
            match reload {
                Some(p) => restart_watcher(&mut active, p, file_tx.clone()),
                None => stop_active(&mut active),
            }
        }
        match file_rx.recv_timeout(Duration::from_millis(400)) {
            Ok(paths) => handle_watch_paths(&state, Some(&app), &paths),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    stop_active(&mut active);
}

pub fn spawn_background(state: SharedState, app: AppHandle, reload_rx: Receiver<Option<PathBuf>>) {
    let (file_tx, file_rx) = mpsc::channel::<Vec<PathBuf>>();
    let tx_loop = file_tx.clone();
    let st = state.clone();
    thread::spawn(move || coordinator_loop(st, app, reload_rx, file_rx, tx_loop));
}
