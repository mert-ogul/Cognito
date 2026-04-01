//! Vault import orchestration: parse → chunk → embed (sidecar RPC) + SQLite.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::IMPORT_PROGRESS;
use crate::services::ingest_graph::{run_graph_extraction, sidecar_purge_graph};
use crate::errors::{CognitoError, CognitoResult};
use crate::models::ImportProgress;
use crate::services::import_job;
use crate::vault::IMPORT_BATCH;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri::Emitter;
use uuid::Uuid;

fn sp(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn emit_job(
    app: &AppHandle,
    job_id: &str,
    status: &str,
    total: u32,
    done: u32,
    failed: u32,
    phase: &str,
    path: Option<&str>,
    err: Option<&str>,
) {
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
            json!({ "sqlitePath": sp(sqlite_path), "chunkIds": ids }),
        )?;
        Ok(())
    })
}

fn remove_existing_doc(
    state: &SharedState,
    conn: &rusqlite::Connection,
    sqlite_path: &Path,
    duck_path: &Path,
    path: &str,
) -> CognitoResult<()> {
    let Some(oid) = sqlite::document_id_by_path(conn, path)? else {
        return Ok(());
    };
    let cids = sqlite::list_chunk_ids_for_document(conn, &oid)?;
    sidecar_purge_graph(state, duck_path, &oid)?;
    sidecar_purge(state, sqlite_path, &cids)?;
    sqlite::delete_document_by_id(conn, &oid)?;
    Ok(())
}

fn parse_batch(state: &SharedState, vault: &Path, batch: &[PathBuf]) -> CognitoResult<Value> {
    let paths: Vec<String> = batch.iter().map(|p| sp(p)).collect();
    state.sidecar.with_session(|s| {
        s.request(
            "parse_documents",
            json!({ "vaultRoot": sp(vault), "paths": paths }),
        )
    })
}

fn chunk_items(state: &SharedState, items: &[Value]) -> CognitoResult<Value> {
    let arr: Vec<Value> = items.to_vec();
    state.sidecar.with_session(|s| s.request("chunk_documents", json!({ "items": arr })))
}

fn embed_items_rpc(
    state: &SharedState,
    sqlite_path: &Path,
    duck: &Path,
    embed: &str,
    chat: &str,
    document_id: &str,
    items: Vec<Value>,
) -> CognitoResult<Value> {
    state.sidecar.with_session(|s| {
        s.request(
            "embed_chunks",
            json!({
                "sqlitePath": sp(sqlite_path),
                "embedModel": embed,
                "duckPath": sp(duck),
                "chatModel": chat,
                "documentId": document_id,
                "items": items,
            }),
        )
    })
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
        _ => ext,
    }
}

fn insert_parse_errors(
    conn: &rusqlite::Connection,
    errors: &[Value],
) -> CognitoResult<u32> {
    let mut n = 0u32;
    for e in errors {
        let Some(p) = e.get("path").and_then(|x| x.as_str()) else {
            continue;
        };
        let msg = e
            .get("error")
            .and_then(|x| x.as_str())
            .unwrap_or("parse error");
        conn.execute("DELETE FROM documents WHERE path = ?1", [p])
            .map_err(|e| CognitoError::Database(e.to_string()))?;
        let pb = PathBuf::from(p);
        let id = Uuid::new_v4().to_string();
        sqlite::insert_document_row(
            conn,
            &id,
            p,
            pb.file_stem().and_then(|s| s.to_str()).unwrap_or("doc"),
            &file_type_label(&pb),
            "",
            None,
            "failed",
            Some(msg),
        )?;
        n += 1;
    }
    Ok(n)
}

fn page_count_opt(v: &Value) -> Option<i64> {
    v.get("pageCount")
        .and_then(|x| x.as_i64())
        .or_else(|| v.get("pageCount").and_then(|x| x.as_u64().map(|u| u as i64)))
}

/// `true` = counted as completed (indexed or idempotent skip); `false` = failed row recorded.
fn process_one_parsed(
    state: &SharedState,
    conn: &rusqlite::Connection,
    sqlite_path: &Path,
    duck_path: &Path,
    embed: &str,
    chat: &str,
    parsed: &Value,
    app: Option<&AppHandle>,
    job_id: Option<&str>,
) -> CognitoResult<bool> {
    let path = parsed
        .get("path")
        .and_then(|x| x.as_str())
        .ok_or_else(|| CognitoError::Sidecar("parse path".into()))?;
    let chash = parsed
        .get("contentHash")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if let Ok(Some(existing)) = sqlite::content_hash_by_path(conn, path) {
        if existing == chash {
            return Ok(true);
        }
    }
    remove_existing_doc(state, conn, sqlite_path, duck_path, path)?;
    let chunked = chunk_items(state, &[parsed.clone()])?;
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
    )?;
    let mut embed_payload: Vec<Value> = Vec::new();
    for ch in chunks {
        let ord = ch.get("ordinal").and_then(|x| x.as_i64()).unwrap_or(0);
        let text = ch
            .get("text")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let c_hash = ch
            .get("contentHash")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let tok = ch.get("tokenCount").and_then(|x| x.as_i64()).unwrap_or(1);
        let ps = ch.get("pageStart").and_then(|x| x.as_i64());
        let pe = ch.get("pageEnd").and_then(|x| x.as_i64());
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
        )?;
        embed_payload.push(json!({
            "chunkId": cid,
            "text": text,
            "ordinal": ord,
        }));
    }
    if embed_payload.is_empty() {
        sqlite::mark_document_indexed(conn, &doc_id)?;
        run_graph_extraction(state, app, job_id, sqlite_path, duck_path, chat, &doc_id)?;
        return Ok(true);
    }
    let emb = embed_items_rpc(
        state,
        sqlite_path,
        duck_path,
        embed,
        chat,
        &doc_id,
        embed_payload,
    )?;
    let failed = emb
        .get("failed")
        .and_then(|x| x.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if failed {
        sqlite::mark_document_failed(conn, &doc_id, "embedding failed for one or more chunks")?;
        return Ok(false);
    }
    sqlite::mark_document_indexed(conn, &doc_id)?;
    run_graph_extraction(state, app, job_id, sqlite_path, duck_path, chat, &doc_id)?;
    Ok(true)
}

/// Runs a full import job on the current thread (spawn from commands).
pub fn run_import_job(
    state: SharedState,
    app: AppHandle,
    job_id: String,
    vault_root: PathBuf,
    paths: Vec<PathBuf>,
    embed_model: String,
    chat_model: String,
) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let duck_path = state.duckdb_path();
    let total = paths.len() as u32;
    state.with_sqlite(|c| import_job::start_job(c, &job_id, paths.len() as i64))?;
    emit_job(
        &app,
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
        emit_job(
            &app,
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
            let path_hint = p.get("path").and_then(|x| x.as_str());
            emit_job(
                &app,
                &job_id,
                "running",
                total,
                done,
                failed,
                "index",
                path_hint,
                None,
            );
            let r = state.with_sqlite(|c| {
                process_one_parsed(
                    &state,
                    c,
                    &sqlite_path,
                    &duck_path,
                    &embed_model,
                    &chat_model,
                    p,
                    Some(&app),
                    Some(&job_id),
                )
            });
            match r {
                Ok(true) => done += 1,
                Ok(false) => failed += 1,
                Err(e) => {
                    failed += 1;
                    let _ = state.with_sqlite(|c| {
                        if let Some(path) = p.get("path").and_then(|x| x.as_str()) {
                            let pb = PathBuf::from(path);
                            let id = Uuid::new_v4().to_string();
                            let msg = e.to_string();
                            let clip = msg.chars().take(512).collect::<String>();
                            sqlite::insert_document_row(
                                c,
                                &id,
                                path,
                                pb.file_stem().and_then(|s| s.to_str()).unwrap_or("doc"),
                                &file_type_label(&pb),
                                "",
                                None,
                                "failed",
                                Some(&clip),
                            )?;
                        }
                        Ok::<(), crate::errors::CognitoError>(())
                    });
                }
            }
            state.with_sqlite(|c| import_job::tick(c, &job_id, done as i64, failed as i64))?;
        }
    }
    state.with_sqlite(|c| import_job::complete_ok(c, &job_id))?;
    emit_job(
        &app,
        &job_id,
        "completed",
        total,
        done,
        failed,
        "done",
        None,
        None,
    );
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
    let duck_path = state.duckdb_path();
    let job_id = "watcher".to_string();
    if let Some(handle) = app {
        emit_job(
            handle,
            &job_id,
            "running",
            1,
            0,
            0,
            "index",
            Some(&sp(file_path)),
            None,
        );
    }
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
        state.with_sqlite(|c| {
            process_one_parsed(
                state,
                c,
                &sqlite_path,
                &duck_path,
                embed_model,
                chat_model,
                p,
                app,
                Some("watcher"),
            )
        })?;
    }
    if let Some(handle) = app {
        emit_job(
            handle,
            &job_id,
            "completed",
            1,
            1,
            0,
            "done",
            Some(&sp(file_path)),
            None,
        );
    }
    Ok(())
}
