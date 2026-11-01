//! Vault import orchestration: parse → chunk → embed (sidecar RPC) + SQLite.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::events::IMPORT_PROGRESS;
use crate::models::ImportProgress;
use crate::services::chat_gate::{self, IMPORT_CHAT_WAIT_CAP};
use crate::services::import_job;
use crate::vault::{self, IMPORT_BATCH};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::AppHandle;
use tauri::Emitter;
use uuid::Uuid;

/// Chunks per `embed_chunks` sidecar request so chat can interleave.
pub const EMBED_RPC_BATCH: usize = 32;

/// Test hook: increments once per `embed_chunks` RPC.
pub static EMBED_RPC_CALLS: AtomicUsize = AtomicUsize::new(0);

fn emit_job(
    app: Option<&AppHandle>,
    job_id: &str,
    status: &str,
    total: u32,
    done: u32,
    failed: u32,
    phase: &str,
    path: Option<&str>,
    err: Option<&str>,
) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(
        IMPORT_PROGRESS,
        &ImportProgress {
            job_id: job_id.to_string(),
            status: status.to_string(),
            total_files: total,
            completed_files: done,
            failed_files: failed,
            phase: phase.to_string(),
            path: path.map(String::from),
            error_message: err.map(String::from),
        },
    );
}

fn sidecar_purge(state: &SharedState, sqlite_path: &Path, ids: &[String]) -> CognitoResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    state.sidecar.with_session(|s| {
        s.request(
            "purge_chunk_embeddings",
            json!({ "sqlitePath": vault::path_string(sqlite_path), "chunkIds": ids }),
        )?;
        Ok(())
    })
}

fn remove_existing_doc(state: &SharedState, sqlite_path: &Path, path: &str) -> CognitoResult<()> {
    let Some(oid) = state.with_sqlite(|conn| sqlite::document_id_including_removed(conn, path))?
    else {
        return Ok(());
    };
    let cids = state.with_sqlite(|conn| sqlite::list_chunk_ids_for_document(conn, &oid))?;
    state.with_sqlite(|conn| sqlite::clear_document_entity_rows(conn, &oid))?;
    sidecar_purge(state, sqlite_path, &cids)?;
    state.with_sqlite(|conn| sqlite::delete_document_by_id(conn, &oid))?;
    Ok(())
}

fn parse_documents_rpc(
    state: &SharedState,
    vault: &Path,
    paths: &[PathBuf],
) -> CognitoResult<Value> {
    let path_strs: Vec<String> = paths.iter().map(|p| vault::path_string(p)).collect();
    state.sidecar.with_session(|s| {
        s.request(
            "parse_documents",
            json!({ "vaultRoot": vault::path_string(vault), "paths": path_strs }),
        )
    })
}

/// Parse a batch; if the RPC fails (e.g. one bad PDF raises in the sidecar), retry per file
/// so successful siblings still index and failures land in `errors`.
fn parse_batch(state: &SharedState, vault: &Path, batch: &[PathBuf]) -> CognitoResult<Value> {
    match parse_documents_rpc(state, vault, batch) {
        Ok(v) => Ok(v),
        Err(e) if batch.len() == 1 => {
            let path = vault::path_string(&batch[0]);
            Ok(json!({
                "items": [],
                "errors": [{ "path": path, "error": e.to_string() }],
            }))
        }
        Err(_) => {
            let mut items: Vec<Value> = Vec::new();
            let mut errors: Vec<Value> = Vec::new();
            for path in batch {
                match parse_documents_rpc(state, vault, std::slice::from_ref(path)) {
                    Ok(v) => {
                        if let Some(its) = v.get("items").and_then(|x| x.as_array()) {
                            items.extend(its.iter().cloned());
                        }
                        if let Some(errs) = v.get("errors").and_then(|x| x.as_array()) {
                            errors.extend(errs.iter().cloned());
                        }
                    }
                    Err(e) => {
                        errors.push(
                            json!({ "path": vault::path_string(path), "error": e.to_string() }),
                        );
                    }
                }
            }
            Ok(json!({ "items": items, "errors": errors }))
        }
    }
}

fn chunk_items(state: &SharedState, items: &[Value]) -> CognitoResult<Value> {
    let arr: Vec<Value> = items.to_vec();
    state
        .sidecar
        .with_session(|s| s.request("chunk_documents", json!({ "items": arr })))
}

fn embed_items_rpc(
    state: &SharedState,
    sqlite_path: &Path,
    embed: &str,
    chat: &str,
    document_id: &str,
    items: Vec<Value>,
) -> CognitoResult<Value> {
    EMBED_RPC_CALLS.fetch_add(1, Ordering::SeqCst);
    state.sidecar.with_session(|s| {
        s.request(
            "embed_chunks",
            json!({
                "sqlitePath": vault::path_string(sqlite_path),
                "embedModel": embed,
                "chatModel": chat,
                "documentId": document_id,
                "items": items,
            }),
        )
    })
}

fn pause_if_chat(
    state: &SharedState,
    app: Option<&AppHandle>,
    job_id: &str,
    total: u32,
    done: u32,
    failed: u32,
) {
    if !chat_gate::chat_in_flight(state) {
        return;
    }
    emit_job(
        app,
        job_id,
        "running",
        total,
        done,
        failed,
        "paused_for_chat",
        None,
        None,
    );
    let _ = chat_gate::wait_until_chat_idle(state, IMPORT_CHAT_WAIT_CAP);
}

fn embed_document_in_batches(
    state: &SharedState,
    app: Option<&AppHandle>,
    job_id: Option<&str>,
    total: u32,
    done: u32,
    failed: u32,
    sqlite_path: &Path,
    embed: &str,
    chat: &str,
    document_id: &str,
    embed_payload: Vec<Value>,
) -> CognitoResult<bool> {
    for batch in embed_payload.chunks(EMBED_RPC_BATCH) {
        if let Some(jid) = job_id {
            pause_if_chat(state, app, jid, total, done, failed);
        } else {
            let _ = chat_gate::wait_until_chat_idle(state, IMPORT_CHAT_WAIT_CAP);
        }
        let emb = embed_items_rpc(
            state,
            sqlite_path,
            embed,
            chat,
            document_id,
            batch.to_vec(),
        )?;
        let batch_failed = emb
            .get("failed")
            .and_then(|x| x.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if batch_failed {
            return Ok(false);
        }
    }
    Ok(true)
}

fn file_type_label(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "pdf".into(),
        "md" | "markdown" => "markdown".into(),
        "docx" => "docx".into(),
        _ => ext,
    }
}

fn insert_parse_errors(conn: &rusqlite::Connection, errors: &[Value]) -> CognitoResult<u32> {
    let mut n = 0u32;
    for e in errors {
        let Some(p) = e.get("path").and_then(|x| x.as_str()) else {
            continue;
        };
        let msg = e
            .get("error")
            .and_then(|x| x.as_str())
            .unwrap_or("parse error");
        let pb = PathBuf::from(p);
        let title = pb.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");
        sqlite::record_failed_document(conn, p, title, &file_type_label(&pb), msg)?;
        n += 1;
    }
    Ok(n)
}

fn page_count_opt(v: &Value) -> Option<i64> {
    v.get("pageCount").and_then(|x| x.as_i64()).or_else(|| {
        v.get("pageCount")
            .and_then(|x| x.as_u64().map(|u| u as i64))
    })
}

/// `true` = counted as completed (indexed or idempotent skip); `false` = failed row recorded.
fn process_one_parsed(
    state: &SharedState,
    sqlite_path: &Path,
    embed: &str,
    chat: &str,
    parsed: &Value,
    app: Option<&AppHandle>,
    job_id: Option<&str>,
    total: u32,
    done: u32,
    failed: u32,
) -> CognitoResult<bool> {
    let path = parsed
        .get("path")
        .and_then(|x| x.as_str())
        .ok_or_else(|| CognitoError::Sidecar("parse path".into()))?;
    let chash = parsed
        .get("contentHash")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let skip = state.with_sqlite(|conn| {
        if let Ok(Some(existing)) = sqlite::content_hash_by_path(conn, path) {
            if existing == chash {
                return Ok(true);
            }
        }
        Ok(false)
    })?;
    if skip {
        return Ok(true);
    }
    remove_existing_doc(state, sqlite_path, path)?;
    let chunked = chunk_items(state, std::slice::from_ref(parsed))?;
    let items = chunked
        .get("items")
        .and_then(|x| x.as_array())
        .ok_or_else(|| CognitoError::Sidecar("chunk items".into()))?;
    let first = items
        .first()
        .ok_or_else(|| CognitoError::Sidecar("empty chunk batch".into()))?;
    let chunks = first
        .get("chunks")
        .and_then(|x| x.as_array())
        .ok_or_else(|| CognitoError::Sidecar("chunks".into()))?;
    let title = parsed
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("doc");
    let ftype = parsed
        .get("fileType")
        .and_then(|x| x.as_str())
        .unwrap_or("markdown");
    let doc_id = Uuid::new_v4().to_string();
    let embed_payload = state.with_sqlite(|conn| {
        let tx = conn.unchecked_transaction()?;
        let scan_status = parsed
            .get("scanStatus")
            .and_then(|x| x.as_str())
            .filter(|s| *s == "ocr" || *s == "no_text" || *s == "partial_text");
        sqlite::insert_document_row(
            conn,
            &doc_id,
            path,
            title,
            ftype,
            chash,
            page_count_opt(parsed),
            "indexing",
            None,
            scan_status,
        )?;
        let mut embed_payload: Vec<Value> = Vec::new();
        for ch in chunks {
            let ord = ch.get("ordinal").and_then(|x| x.as_i64()).unwrap_or(0);
            let text = ch
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let c_hash = ch.get("contentHash").and_then(|x| x.as_str()).unwrap_or("");
            let tok = ch.get("tokenCount").and_then(|x| x.as_i64()).unwrap_or(1);
            let ps = ch.get("pageStart").and_then(|x| x.as_i64());
            let pe = ch.get("pageEnd").and_then(|x| x.as_i64());
            let role = ch.get("role").and_then(|x| x.as_str()).unwrap_or("body");
            let labels = match ch.get("labels") {
                None => "[]".to_string(),
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
            };
            let regions = match ch.get("regions") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => {
                    if s.is_empty() || s == "null" {
                        None
                    } else {
                        Some(s.clone())
                    }
                }
                Some(v) => Some(v.to_string()),
            };
            let text_quality = ch.get("textQuality").and_then(|x| x.as_f64());
            let searchable = ch
                .get("searchable")
                .and_then(|x| x.as_bool())
                .unwrap_or(true);
            let cid = Uuid::new_v4().to_string();
            sqlite::insert_chunk_row(
                conn,
                &cid,
                &doc_id,
                ord,
                ps,
                pe,
                &text,
                tok,
                c_hash,
                role,
                &labels,
                regions.as_deref(),
                text_quality,
            )?;
            if !searchable {
                continue;
            }
            let heading_trail = ch
                .get("headingTrail")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty());
            let mut item = json!({
                "chunkId": cid,
                "text": text,
                "ordinal": ord,
                "title": title,
                "searchable": true,
            });
            if let Some(q) = text_quality {
                item.as_object_mut()
                    .expect("embed item object")
                    .insert("textQuality".into(), json!(q));
            }
            if let Some(ht) = heading_trail {
                item.as_object_mut()
                    .expect("embed item object")
                    .insert("headingTrail".into(), json!(ht));
            }
            embed_payload.push(item);
        }
        tx.commit()?;
        Ok(embed_payload)
    })?;
    if embed_payload.is_empty() {
        state.with_sqlite(|conn| sqlite::mark_document_indexed(conn, &doc_id))?;
        return Ok(true);
    }
    let ok = embed_document_in_batches(
        state,
        app,
        job_id,
        total,
        done,
        failed,
        sqlite_path,
        embed,
        chat,
        &doc_id,
        embed_payload,
    )?;
    if !ok {
        state.with_sqlite(|conn| {
            sqlite::mark_document_failed(conn, &doc_id, "embedding failed for one or more chunks")
        })?;
        return Ok(false);
    }
    state.with_sqlite(|conn| {
        sqlite::mark_document_indexed(conn, &doc_id)?;
        sqlite::maybe_set_index_embed_model(conn, embed)?;
        Ok(())
    })?;
    Ok(true)
}

/// Runs a full import job on the current thread (spawn from commands).
pub fn run_import_job(
    state: SharedState,
    app: Option<AppHandle>,
    job_id: String,
    vault_root: PathBuf,
    paths: Vec<PathBuf>,
    embed_model: String,
    chat_model: String,
) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let total = paths.len() as u32;
    state.with_sqlite(|c| import_job::start_job(c, &job_id, paths.len() as i64))?;
    emit_job(
        app.as_ref(),
        &job_id,
        "running",
        total,
        0,
        0,
        "start",
        None,
        None,
    );
    let mut done = 0u32;
    let mut failed = 0u32;
    for batch in paths.chunks(IMPORT_BATCH) {
        pause_if_chat(&state, app.as_ref(), &job_id, total, done, failed);
        emit_job(
            app.as_ref(),
            &job_id,
            "running",
            total,
            done,
            failed,
            "parse",
            None,
            None,
        );
        let parsed = parse_batch(&state, &vault_root, batch)?;
        if let Some(errs) = parsed.get("errors").and_then(|x| x.as_array()) {
            let n = state.with_sqlite(|c| insert_parse_errors(c, errs))?;
            failed += n;
            state.with_sqlite(|c| import_job::tick(c, &job_id, done as i64, failed as i64))?;
        }
        let items = parsed
            .get("items")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        for p in &items {
            pause_if_chat(&state, app.as_ref(), &job_id, total, done, failed);
            let path_hint = p.get("path").and_then(|x| x.as_str());
            emit_job(
                app.as_ref(),
                &job_id,
                "running",
                total,
                done,
                failed,
                "index",
                path_hint,
                None,
            );
            let r = process_one_parsed(
                &state,
                &sqlite_path,
                &embed_model,
                &chat_model,
                p,
                app.as_ref(),
                Some(&job_id),
                total,
                done,
                failed,
            );
            match r {
                Ok(true) => done += 1,
                Ok(false) => failed += 1,
                Err(e) => {
                    failed += 1;
                    let _ = state.with_sqlite(|c| {
                        if let Some(path) = p.get("path").and_then(|x| x.as_str()) {
                            let pb = PathBuf::from(path);
                            let msg = e.to_string();
                            sqlite::record_failed_document(
                                c,
                                path,
                                pb.file_stem().and_then(|s| s.to_str()).unwrap_or("doc"),
                                &file_type_label(&pb),
                                &msg,
                            )?;
                        }
                        Ok::<(), CognitoError>(())
                    });
                }
            }
            state.with_sqlite(|c| import_job::tick(c, &job_id, done as i64, failed as i64))?;
        }
    }
    state.with_sqlite(|c| {
        import_job::complete_ok(c, &job_id)?;
        sqlite::upsert_setting(
            c,
            "index_format_version",
            &sqlite::INDEX_FORMAT_VERSION.to_string(),
        )?;
        Ok(())
    })?;
    state
        .index_format_updating
        .store(false, std::sync::atomic::Ordering::SeqCst);
    emit_job(
        app.as_ref(),
        &job_id,
        "completed",
        total,
        done,
        failed,
        "done",
        None,
        None,
    );
    if let Some(ref a) = app {
        let _ = a.emit(crate::events::LIBRARY_CHANGED, &());
    }
    state.notify_background_worker();
    Ok(())
}

/// Watcher / single-file incremental re-index.
pub fn run_single_file(
    state: &SharedState,
    app: Option<&AppHandle>,
    vault_root: &Path,
    file_path: &Path,
    embed_model: &str,
    chat_model: &str,
) -> CognitoResult<()> {
    if !crate::vault::is_importable_extension(file_path) {
        return Ok(());
    }
    let sqlite_path = state.sqlite_path();
    let job_id = "watcher".to_string();
    emit_job(
        app,
        &job_id,
        "running",
        1,
        0,
        0,
        "index",
        Some(&vault::path_string(file_path)),
        None,
    );
    let parsed = parse_batch(state, vault_root, &[file_path.to_path_buf()])?;
    if let Some(errs) = parsed.get("errors").and_then(|x| x.as_array()) {
        if !errs.is_empty() {
            state.with_sqlite(|c| insert_parse_errors(c, errs))?;
        }
    }
    let items = parsed
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    if let Some(p) = items.first() {
        pause_if_chat(state, app, &job_id, 1, 0, 0);
        process_one_parsed(
            state,
            &sqlite_path,
            embed_model,
            chat_model,
            p,
            app,
            Some(&job_id),
            1,
            0,
            0,
        )?;
    }
    emit_job(
        app,
        &job_id,
        "completed",
        1,
        1,
        0,
        "done",
        Some(&vault::path_string(file_path)),
        None,
    );
    state.notify_background_worker();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::services::background_worker;
    use crate::services::file_watcher;
    use crate::sidecar::SidecarPool;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    #[ignore = "needs the sidecar venv: cd sidecar && uv sync"]
    fn import_job_indexes_md_and_fails_broken_pdf_without_deadlock() {
        std::env::set_var("COGNITO_MOCK_EMBED", "1");
        std::env::set_var("COGNITO_MOCK_LLM", "1");
        std::env::set_var("COGNITO_MOCK_MODELS", "1");

        let data = tempfile::tempdir().expect("data tempdir");
        let vault = tempfile::tempdir().expect("vault tempdir");

        let note_path = vault.path().join("note.md");
        std::fs::write(
            &note_path,
            "First paragraph about Ada Chen at Meridian Labs.\n\n\
             Second paragraph mentions Priya Nair in Cambridge.\n\n\
             Third paragraph covers Jonah Hale in Boston.\n",
        )
        .expect("write note.md");

        let broken_path = vault.path().join("broken.pdf");
        std::fs::write(&broken_path, "This is not a real PDF file.").expect("write broken.pdf");

        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.path().join("logs").join("sidecar.log");
        std::fs::create_dir_all(log_path.parent().unwrap()).expect("logs dir");
        let pool = SidecarPool::new(main_py, sidecar_cwd, log_path, data.path().to_path_buf());
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state.sidecar.ensure().expect("sidecar ensure");

        let note_abs = note_path
            .canonicalize()
            .unwrap_or_else(|_| note_path.clone());
        let broken_abs = broken_path
            .canonicalize()
            .unwrap_or_else(|_| broken_path.clone());
        let paths = vec![note_abs.clone(), broken_abs.clone()];
        let vault_root = vault.path().to_path_buf();
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "vault_path", &vault::path_string(&vault_root))?;
                sqlite::upsert_setting(c, "extract_entities", "true")?;
                sqlite::upsert_setting(c, "summarise_documents", "false")?;
                Ok(())
            })
            .expect("vault setting");

        let (tx, rx) = mpsc::channel();
        let st = state.clone();
        let paths_run = paths.clone();
        let vault_run = vault_root.clone();
        std::thread::spawn(move || {
            let res = run_import_job(
                st,
                None,
                "test-job-1".into(),
                vault_run,
                paths_run,
                "nomic-embed-text".into(),
                "phi4-mini".into(),
            );
            let _ = tx.send(res);
        });
        let res = rx
            .recv_timeout(Duration::from_secs(180))
            .expect("import timed out after 180s");
        res.expect("first import");

        let note_s = vault::path_string(&note_abs);
        let broken_s = vault::path_string(&broken_abs);
        state
            .with_sqlite(|conn| {
                let note_status: String = conn
                    .query_row(
                        "SELECT status FROM documents WHERE path = ?1",
                        [&note_s],
                        |r| r.get(0),
                    )?;
                assert_eq!(note_status, "indexed");
                let entities_status: String = conn
                    .query_row(
                        "SELECT entities_status FROM documents WHERE path = ?1",
                        [&note_s],
                        |r| r.get(0),
                    )?;
                assert_eq!(entities_status, "pending");
                let chunk_n: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.path = ?1",
                        [&note_s],
                        |r| r.get(0),
                    )?;
                assert!(chunk_n >= 1, "expected at least one chunk");
                let broken_status: String = conn
                    .query_row(
                        "SELECT status FROM documents WHERE path = ?1",
                        [&broken_s],
                        |r| r.get(0),
                    )?;
                assert_eq!(broken_status, "failed");
                Ok(())
            })
            .expect("assert first import rows");

        let processed = background_worker::process_pending(&state, None);
        assert!(
            processed.finished >= 1,
            "expected process_pending to handle note.md"
        );
        state
            .with_sqlite(|conn| {
                let entities_status: String = conn.query_row(
                    "SELECT entities_status FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| r.get(0),
                )?;
                assert_eq!(entities_status, "ok");
                Ok(())
            })
            .expect("entities ok after process_pending");

        let index_model = state
            .with_sqlite(|c| sqlite::get_setting(c, "index_embed_model"))
            .expect("index_embed_model")
            .expect("index_embed_model set after import");
        assert_eq!(index_model, "nomic-embed-text");

        crate::services::library::reset(&state).expect("reset");
        state
            .with_sqlite(|conn| {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
                assert_eq!(n, 0);
                let vec_n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'chunk_embeddings'",
                    [],
                    |r| r.get(0),
                )?;
                assert_eq!(vec_n, 0);
                assert!(sqlite::get_setting(conn, "index_embed_model")?.is_none());
                Ok(())
            })
            .expect("empty after reset");

        let (tx_r, rx_r) = mpsc::channel();
        let st_r = state.clone();
        let paths_r = paths.clone();
        let vault_r = vault_root.clone();
        std::thread::spawn(move || {
            let res = run_import_job(
                st_r,
                None,
                "test-job-rebuild".into(),
                vault_r,
                paths_r,
                "nomic-embed-text".into(),
                "phi4-mini".into(),
            );
            let _ = tx_r.send(res);
        });
        rx_r.recv_timeout(Duration::from_secs(180))
            .expect("rebuild import timed out")
            .expect("rebuild import");
        state
            .with_sqlite(|conn| {
                let note_status: String = conn.query_row(
                    "SELECT status FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| r.get(0),
                )?;
                assert_eq!(note_status, "indexed");
                Ok(())
            })
            .expect("re-indexed after reset");

        let removed_path = vault.path().join("skip-me.md");
        std::fs::write(&removed_path, "Skip this note.\n\nSecond.\n\nThird.\n").expect("skip md");
        let removed_abs = removed_path
            .canonicalize()
            .unwrap_or_else(|_| removed_path.clone());
        run_single_file(
            &state,
            None,
            &vault_root,
            &removed_abs,
            "nomic-embed-text",
            "phi4-mini",
        )
        .expect("index skip-me");
        let removed_s = vault::path_string(&removed_abs);
        state
            .with_sqlite(|c| {
                sqlite::mark_document_removed(c, &removed_s)?;
                Ok(())
            })
            .expect("mark removed");
        let _ = background_worker::process_pending(&state, None);

        // Lock still usable from the test thread (no permanent deadlock).
        state
            .with_sqlite(|_| Ok(()))
            .expect("with_sqlite after import");

        let (tx2, rx2) = mpsc::channel();
        let st2 = state.clone();
        let paths_run2 = paths.clone();
        let vault_run2 = vault_root.clone();
        std::thread::spawn(move || {
            let res = run_import_job(
                st2,
                None,
                "test-job-2".into(),
                vault_run2,
                paths_run2,
                "nomic-embed-text".into(),
                "phi4-mini".into(),
            );
            let _ = tx2.send(res);
        });
        let res2 = rx2
            .recv_timeout(Duration::from_secs(180))
            .expect("second import timed out after 180s");
        res2.expect("second import");

        state
            .with_sqlite(|conn| {
                let note_n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| r.get(0),
                )?;
                let broken_n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM documents WHERE path = ?1",
                    [&broken_s],
                    |r| r.get(0),
                )?;
                assert_eq!(note_n, 1);
                assert_eq!(broken_n, 1);
                let note_status: String = conn.query_row(
                    "SELECT status FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| r.get(0),
                )?;
                assert_eq!(note_status, "indexed");
                Ok(())
            })
            .expect("assert second import rows");

        std::fs::write(
            &note_abs,
            "Updated note about Ada Chen and Meridian Labs in Cambridge.\n\n\
             Second paragraph still mentions Priya Nair.\n\n\
             Third paragraph still covers Jonah Hale in Boston.\n",
        )
        .expect("rewrite note.md");
        run_single_file(
            &state,
            None,
            &vault_root,
            &note_abs,
            "nomic-embed-text",
            "phi4-mini",
        )
        .expect("reindex note");
        state
            .with_sqlite(|conn| {
                let (status, estat): (String, String) = conn.query_row(
                    "SELECT status, entities_status FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                assert_eq!(status, "indexed");
                assert_eq!(estat, "pending");
                Ok(())
            })
            .expect("assert reindex pending");
        let _ = background_worker::process_pending(&state, None);
        state
            .with_sqlite(|conn| {
                let estat: String = conn.query_row(
                    "SELECT entities_status FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| r.get(0),
                )?;
                assert_eq!(estat, "ok");
                Ok(())
            })
            .expect("assert reindex entities ok");

        std::fs::remove_file(&note_abs).expect("delete note.md");
        file_watcher::handle_watch_paths(&state, None, std::slice::from_ref(&note_abs));
        state
            .with_sqlite(|conn| {
                let status: String = conn.query_row(
                    "SELECT status FROM documents WHERE path = ?1",
                    [&note_s],
                    |r| r.get(0),
                )?;
                assert_eq!(status, "removed");
                Ok(())
            })
            .expect("assert removed");

        let vault_b = tempfile::tempdir().expect("vault B");
        let b_note = vault_b.path().join("only-b.md");
        std::fs::write(
            &b_note,
            "Folder B note about Meridian Labs.\n\nSecond paragraph.\n\nThird paragraph.\n",
        )
        .expect("write only-b.md");
        let b_paths =
            crate::services::library::switch_vault(&state, &vault_b.path().to_string_lossy())
                .expect("switch_vault");
        state
            .with_sqlite(|conn| {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
                assert_eq!(n, 0, "switch_vault must clear documents");
                let vp = sqlite::get_setting(conn, "vault_path")?.expect("vault_path after switch");
                assert_eq!(PathBuf::from(&vp), vault_b.path());
                Ok(())
            })
            .expect("assert switch_vault");
        assert_eq!(b_paths.len(), 1);
        assert_eq!(
            b_paths[0].file_name().and_then(|n| n.to_str()),
            Some("only-b.md")
        );
        let b_abs = b_note.canonicalize().unwrap_or_else(|_| b_note.clone());

        let (tx_b, rx_b) = mpsc::channel();
        let st_b = state.clone();
        let paths_b = b_paths.clone();
        let vault_b_root = vault_b.path().to_path_buf();
        std::thread::spawn(move || {
            let res = run_import_job(
                st_b,
                None,
                "test-job-b".into(),
                vault_b_root,
                paths_b,
                "nomic-embed-text".into(),
                "phi4-mini".into(),
            );
            let _ = tx_b.send(res);
        });
        rx_b.recv_timeout(Duration::from_secs(180))
            .expect("B import timed out")
            .expect("B import");
        state
            .with_sqlite(|conn| {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
                assert_eq!(n, 1);
                let path: String =
                    conn.query_row("SELECT path FROM documents", [], |r| r.get(0))?;
                assert_eq!(path, vault::path_string(&b_abs));
                Ok(())
            })
            .expect("only B indexed");

        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "vault_path", "/nonexistent/cognito-vault")?;
                Ok(())
            })
            .expect("bad vault_path");
        let rebuild_err = crate::services::library::plan_rebuild(&state);
        assert!(
            rebuild_err.is_err(),
            "rebuild with missing vault must error"
        );
        state
            .with_sqlite(|conn| {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
                assert_eq!(n, 1, "failed rebuild must leave documents");
                Ok(())
            })
            .expect("docs after failed rebuild");

        crate::services::library::forget(&state).expect("forget");
        state
            .with_sqlite(|conn| {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
                assert_eq!(n, 0);
                assert!(sqlite::get_setting(conn, "vault_path")?.is_none());
                Ok(())
            })
            .expect("empty after forget");
    }

    #[test]
    fn embed_rpc_batch_is_32_and_100_chunks_need_four_requests() {
        assert_eq!(EMBED_RPC_BATCH, 32);
        let n = 100usize;
        let batches = n.div_ceil(EMBED_RPC_BATCH);
        assert_eq!(batches, 4);
    }

    #[test]
    #[ignore = "needs the sidecar venv: cd sidecar && uv sync"]
    fn embed_batches_100_chunks_into_four_rpc_calls() {
        std::env::set_var("COGNITO_MOCK_EMBED", "1");

        let data = tempfile::tempdir().expect("data");
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.path().join("logs").join("sidecar.log");
        std::fs::create_dir_all(log_path.parent().unwrap()).expect("logs");
        let pool = SidecarPool::new(main_py, sidecar_cwd, log_path, data.path().to_path_buf());
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state.sidecar.ensure().expect("sidecar");

        let payload: Vec<Value> = (0..100)
            .map(|i| {
                json!({
                    "chunkId": format!("c{i}"),
                    "text": format!("chunk body {i}"),
                    "ordinal": i,
                    "title": "Doc",
                })
            })
            .collect();

        EMBED_RPC_CALLS.store(0, Ordering::SeqCst);
        let ok = embed_document_in_batches(
            &state,
            None,
            None,
            0,
            0,
            0,
            &state.sqlite_path(),
            "nomic-embed-text",
            "phi4-mini",
            "doc-1",
            payload,
        )
        .expect("embed batches");
        assert!(ok);
        assert_eq!(EMBED_RPC_CALLS.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn import_waits_while_chat_in_flight_then_continues() {
        let data = tempfile::tempdir().expect("data");
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.path().join("logs").join("sidecar.log");
        std::fs::create_dir_all(log_path.parent().unwrap()).expect("logs");
        let pool = SidecarPool::new(
            main_py,
            sidecar_cwd,
            log_path,
            data.path().to_path_buf(),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));

        state.chat_in_flight.fetch_add(1, Ordering::SeqCst);
        let st = state.clone();
        let waiter = std::thread::spawn(move || {
            chat_gate::wait_until_chat_idle(&st, Duration::from_secs(5))
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !waiter.is_finished(),
            "waiter should block while chat_in_flight > 0"
        );
        state.chat_in_flight.fetch_sub(1, Ordering::SeqCst);
        let cleared = waiter.join().expect("join");
        assert!(cleared);
    }
}
