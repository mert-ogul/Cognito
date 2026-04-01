//! Graph extraction step during ingestion (sidecar RPC + SQLite status).

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::GRAPH_EXTRACTION_PROGRESS;
use crate::errors::CognitoResult;
use crate::models::GraphExtractionProgress;
use serde_json::json;
use std::path::Path;
use tauri::AppHandle;
use tauri::Emitter;

fn sp(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn sidecar_purge_graph(state: &SharedState, duck_path: &Path, document_id: &str) -> CognitoResult<()> {
    state.sidecar.with_session(|s| {
        s.request(
            "purge_graph_document",
            json!({ "duckPath": sp(duck_path), "documentId": document_id }),
        )?;
        Ok(())
    })
}

pub(crate) fn emit_graph(
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
        GRAPH_EXTRACTION_PROGRESS,
        &GraphExtractionProgress {
            job_id: job_id.map(String::from),
            document_id: document_id.to_string(),
            phase: phase.to_string(),
            detail: detail.map(String::from),
        },
    );
}

pub(crate) fn run_graph_extraction(
    state: &SharedState,
    app: Option<&AppHandle>,
    job_id: Option<&str>,
    sqlite_path: &Path,
    duck_path: &Path,
    chat: &str,
    document_id: &str,
) -> CognitoResult<()> {
    emit_graph(app, job_id, document_id, "extract", None);
    let res = state.sidecar.with_session(|s| {
        s.request(
            "extract_graph_document",
            json!({
                "sqlitePath": sp(sqlite_path),
                "duckPath": sp(duck_path),
                "documentId": document_id,
                "chatModel": chat,
            }),
        )
    });
    match res {
        Ok(v) => {
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            if ok {
                state.with_sqlite(|c| sqlite::mark_document_graph_ok(c, document_id))?;
                emit_graph(app, job_id, document_id, "ok", None);
            } else {
                let err = v
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("graph extraction failed");
                state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, document_id, err))?;
                emit_graph(app, job_id, document_id, "failed", Some(err));
            }
        }
        Err(e) => {
            let msg = e.to_string();
            state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, document_id, &msg))?;
            emit_graph(app, job_id, document_id, "failed", Some(&msg));
        }
    }
    Ok(())
}
