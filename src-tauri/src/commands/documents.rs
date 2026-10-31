use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::events::{IMPORT_PROGRESS, LIBRARY_CHANGED};
use crate::models::{
    DocumentCardPayload, DocumentPreview, DocumentRow, ImportProgress, PageRect, PageRenderResult,
    SuggestedQuestion,
};
use crate::services::import_job;
use crate::services::ingest;
use crate::services::library;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::run_blocking;

fn parse_regions_json(raw: Option<String>) -> Option<Value> {
    let s = raw?.trim().to_string();
    if s.is_empty() || s == "null" {
        return None;
    }
    serde_json::from_str(&s).ok()
}

fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn png_dimensions(data: &[u8]) -> CognitoResult<(i64, i64)> {
    if data.len() < 24 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(CognitoError::Sidecar("invalid png cache".into()));
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as i64;
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as i64;
    Ok((width, height))
}

fn read_cached_png(path: &Path) -> CognitoResult<PageRenderResult> {
    let data = std::fs::read(path).map_err(|e| CognitoError::Sidecar(e.to_string()))?;
    let (width, height) = png_dimensions(&data)?;
    Ok(PageRenderResult {
        png_base64: encode_base64(&data),
        width,
        height,
    })
}

fn rect_short_hash(rect: &PageRect) -> String {
    let raw = format!(
        "{:.4},{:.4},{:.4},{:.4}",
        rect.x0, rect.y0, rect.x1, rect.y1
    );
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in raw.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:012x}")
}

pub(crate) fn page_cache_path(
    data_dir: &Path,
    document_id: &str,
    content_hash: &str,
    page: i64,
    scale: f64,
    rect: Option<&PageRect>,
) -> PathBuf {
    let hash12: String = content_hash.chars().take(12).collect();
    let scale_i = (scale * 100.0).round() as i64;
    let mut name = format!("{document_id}-{hash12}-p{page}-s{scale_i}");
    if let Some(r) = rect {
        name.push('-');
        name.push_str(&rect_short_hash(r));
    }
    name.push_str(".png");
    data_dir.join("page_cache").join(name)
}

fn render_document_image(
    state: &SharedState,
    document_id: &str,
    page: i64,
    rect: Option<&PageRect>,
    scale: Option<f64>,
) -> CognitoResult<PageRenderResult> {
    let (path, content_hash) = state
        .with_sqlite(|c| sqlite::document_path_and_hash_by_id(c, document_id))?
        .ok_or_else(|| CognitoError::Vault("Document not found.".into()))?;
    let scale = scale.filter(|s| *s > 0.0).unwrap_or(2.0);
    let cache_path = page_cache_path(
        &state.data_dir,
        document_id,
        &content_hash,
        page,
        scale,
        rect,
    );
    if cache_path.is_file() {
        return read_cached_png(&cache_path);
    }
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CognitoError::Sidecar(e.to_string()))?;
    }
    let method = if rect.is_some() {
        "render_region"
    } else {
        "render_page"
    };
    let mut params = json!({
        "path": path,
        "page": page,
        "scale": scale,
        "outPath": cache_path.to_string_lossy(),
        "app_data": state.data_dir.to_string_lossy(),
    });
    if let Some(r) = rect {
        params.as_object_mut().expect("params object").insert(
            "rect".into(),
            json!([r.x0, r.y0, r.x1, r.y1]),
        );
    }
    let _ = state.sidecar.with_session(|s| s.request(method, params))?;
    if !cache_path.is_file() {
        return Err(CognitoError::Sidecar("render wrote no cache file".into()));
    }
    read_cached_png(&cache_path)
}

#[tauri::command]
pub async fn list_documents(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<DocumentRow>, String> {
    run_blocking(state, |st| st.with_sqlite(|c| sqlite::list_documents(c))).await
}

#[tauri::command]
pub async fn get_document_card(
    state: tauri::State<'_, SharedState>,
    document_id: String,
) -> Result<Option<DocumentCardPayload>, String> {
    run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::get_document_card(c, &document_id))
    })
    .await
}

#[tauri::command]
pub async fn get_suggested_questions(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<SuggestedQuestion>, String> {
    run_blocking(state, |st| st.with_sqlite(|c| sqlite::get_suggested_questions(c))).await
}

#[tauri::command]
pub async fn get_document_preview(
    state: tauri::State<'_, SharedState>,
    document_id: String,
    chunk_id: Option<String>,
) -> Result<DocumentPreview, String> {
    run_blocking(state, move |st| {
        let path = st.with_sqlite(|c| sqlite::document_path_by_id(c, &document_id))?;
        let scan_status = st.with_sqlite(|c| sqlite::document_scan_status(c, &document_id))?;
        if let Some(ref cid) = chunk_id {
            let win = st.with_sqlite(|c| sqlite::chunk_window(c, &document_id, cid))?;
            if let Some(w) = win {
                return Ok(DocumentPreview {
                    document_id,
                    text: Some(w.text),
                    highlight_snippet: Some(w.highlight),
                    page_start: w.page_start,
                    page_end: w.page_end,
                    path,
                    regions: parse_regions_json(w.regions),
                    scan_status,
                });
            }
        }
        let text = st.with_sqlite(|c| sqlite::document_preview_text(c, &document_id))?;
        Ok(DocumentPreview {
            document_id,
            text,
            highlight_snippet: None,
            page_start: None,
            page_end: None,
            path,
            regions: None,
            scan_status,
        })
    })
    .await
}

#[tauri::command]
pub async fn render_page_region(
    state: tauri::State<'_, SharedState>,
    document_id: String,
    page: i64,
    rect: PageRect,
    scale: Option<f64>,
) -> Result<PageRenderResult, String> {
    run_blocking(state, move |st| {
        render_document_image(&st, &document_id, page, Some(&rect), scale)
    })
    .await
}

#[tauri::command]
pub async fn render_document_page(
    state: tauri::State<'_, SharedState>,
    document_id: String,
    page: i64,
    scale: Option<f64>,
) -> Result<PageRenderResult, String> {
    run_blocking(state, move |st| {
        render_document_image(&st, &document_id, page, None, scale)
    })
    .await
}

#[tauri::command]
pub async fn open_document(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    document_id: String,
) -> Result<(), String> {
    let path = state
        .with_sqlite(|c| sqlite::document_path_by_id(c, &document_id))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Document not found.".to_string())?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) struct ImportLockGuard {
    state: SharedState,
}

impl Drop for ImportLockGuard {
    fn drop(&mut self) {
        self.state.import_running.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn try_lock_import(state: &SharedState) -> Result<ImportLockGuard, String> {
    if state.import_running.swap(true, Ordering::SeqCst) {
        return Err("An import is already running.".to_string());
    }
    Ok(ImportLockGuard {
        state: state.clone(),
    })
}

/// Assumes the import lock is held; releases it when the thread ends.
pub(crate) fn spawn_import_thread(
    app: AppHandle,
    guard: ImportLockGuard,
    job_id: String,
    vault: PathBuf,
    paths: Vec<PathBuf>,
    embed: String,
    chat: String,
) {
    let st = guard.state.clone();
    let app_c = app.clone();
    let jid = job_id.clone();
    std::thread::spawn(move || {
        let _guard = guard;
        let res = ingest::run_import_job(
            st.clone(),
            Some(app_c.clone()),
            jid.clone(),
            vault,
            paths,
            embed,
            chat,
        );
        if let Err(ref e) = res {
            let _ = st.with_sqlite(|c| import_job::complete_fail(c, &jid, &e.to_string()));
            let _ = app_c.emit(
                IMPORT_PROGRESS,
                &ImportProgress {
                    job_id: jid.clone(),
                    status: "failed".into(),
                    total_files: 0,
                    completed_files: 0,
                    failed_files: 0,
                    phase: "error".into(),
                    path: None,
                    error_message: Some(e.to_string()),
                },
            );
            let _ = app_c.emit(LIBRARY_CHANGED, &());
        }
        st.index_format_updating.store(false, Ordering::SeqCst);
    });
}

/// If documents exist and `index_format_version` is behind, rebuild like Settings → Rebuild index.
pub(crate) fn maybe_rebuild_stale_index_format(app: AppHandle, state: SharedState) {
    std::thread::spawn(move || {
        let _ = state.sidecar.ensure();
        let need = match state.with_sqlite(|c| {
            let n = sqlite::count_active_documents(c)?;
            if n == 0 {
                return Ok(false);
            }
            let stored = sqlite::get_setting(c, "index_format_version")?
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            Ok(stored < sqlite::INDEX_FORMAT_VERSION)
        }) {
            Ok(v) => v,
            Err(_) => return,
        };
        if !need {
            return;
        }
        let Ok(guard) = try_lock_import(&state) else {
            return;
        };
        state.index_format_updating.store(true, Ordering::SeqCst);
        let (vault, paths) = match library::plan_rebuild(&state) {
            Ok(v) => v,
            Err(_) => {
                state.index_format_updating.store(false, Ordering::SeqCst);
                return;
            }
        };
        if library::reset(&state).is_err() {
            state.index_format_updating.store(false, Ordering::SeqCst);
            return;
        }
        let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
            Ok(s) => s,
            Err(_) => {
                state.index_format_updating.store(false, Ordering::SeqCst);
                return;
            }
        };
        let job_id = Uuid::new_v4().to_string();
        spawn_import_thread(
            app,
            guard,
            job_id,
            vault,
            paths,
            settings.embed_model.clone(),
            settings.chat_model.clone(),
        );
    });
}

#[tauri::command]
pub async fn import_documents(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    folder: Option<String>,
) -> Result<String, String> {
    let guard = try_lock_import(&*state)?;
    let job_id = Uuid::new_v4().to_string();
    let settings = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let vault_str = settings
        .vault_path
        .clone()
        .ok_or_else(|| "Configure a vault folder first.".to_string())?;
    let vault = PathBuf::from(&vault_str);
    let import_root = if let Some(ref f) = folder {
        let fp = PathBuf::from(f);
        crate::vault::ensure_folder_in_vault(&vault, &fp).map_err(|e| e.to_string())?
    } else {
        vault.clone()
    };
    let active = state
        .with_sqlite(|c| sqlite::count_active_documents(c))
        .map_err(|e| e.to_string())?;
    let slots = crate::vault::slots_remaining(active);
    if slots == 0 {
        return Err("Maximum of 100 active documents reached.".to_string());
    }
    let paths = crate::vault::collect_import_paths_bounded(&import_root, slots)
        .map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Err("No PDF, Word or Markdown files found.".to_string());
    }
    spawn_import_thread(
        app,
        guard,
        job_id.clone(),
        vault,
        paths,
        settings.embed_model.clone(),
        settings.chat_model.clone(),
    );
    Ok(job_id)
}

#[tauri::command]
pub async fn retry_failed_import(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    let guard = try_lock_import(&*state)?;
    let job_id = Uuid::new_v4().to_string();
    let settings = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let vault_str = settings
        .vault_path
        .clone()
        .ok_or_else(|| "Configure a vault folder first.".to_string())?;
    let vault = PathBuf::from(&vault_str);
    let entities_n = state
        .with_sqlite(|c| sqlite::requeue_failed_entities(c))
        .map_err(|e| e.to_string())?;
    let paths_str = state
        .with_sqlite(|c| sqlite::failed_document_paths(c, 100))
        .map_err(|e| e.to_string())?;
    if paths_str.is_empty() && entities_n == 0 {
        return Err("No failed documents to retry.".to_string());
    }
    if entities_n > 0 {
        state.notify_background_worker();
    }
    if !paths_str.is_empty() {
        let paths: Vec<PathBuf> = paths_str.iter().map(PathBuf::from).collect();
        spawn_import_thread(
            app,
            guard,
            job_id.clone(),
            vault,
            paths,
            settings.embed_model.clone(),
            settings.chat_model.clone(),
        );
    }
    Ok(job_id)
}

#[tauri::command]
pub async fn rebuild_index(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    embed_model: Option<String>,
) -> Result<String, String> {
    let guard = try_lock_import(&*state)?;
    let (vault, paths) = library::plan_rebuild(&*state).map_err(|e| e.to_string())?;
    if let Some(ref model) = embed_model {
        state
            .with_sqlite(|c| sqlite::upsert_setting(c, "embed_model", model))
            .map_err(|e| e.to_string())?;
    }
    library::reset(&*state).map_err(|e| e.to_string())?;
    let settings = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let job_id = Uuid::new_v4().to_string();
    spawn_import_thread(
        app,
        guard,
        job_id.clone(),
        vault,
        paths,
        settings.embed_model.clone(),
        settings.chat_model.clone(),
    );
    Ok(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::sidecar::SidecarPool;
    use std::sync::Arc;

    // iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xFC,
        0xCF, 0xC0, 0x50, 0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xA9, 0x8C, 0x21, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn test_state(data: &Path) -> SharedState {
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.join("logs").join("sidecar.log");
        std::fs::create_dir_all(log_path.parent().unwrap()).expect("logs");
        let pool = SidecarPool::new(main_py, sidecar_cwd, log_path, data.to_path_buf());
        Arc::new(AppState::new(data.to_path_buf(), pool))
    }

    #[test]
    fn render_unknown_document_id_fails() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let state = test_state(dir.path());
        let err = render_document_image(&state, "missing-id", 1, None, Some(2.0));
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.to_lowercase().contains("not found"), "{msg}");
    }

    #[test]
    fn second_render_reads_cache_without_sidecar() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let state = test_state(dir.path());
        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c,
                    "doc1",
                    "/vault/a.pdf",
                    "A",
                    "pdf",
                    "contenthashabc123",
                    Some(2),
                    "indexed",
                    None,
                    None,
                )
            })
            .expect("insert");

        let cache = page_cache_path(dir.path(), "doc1", "contenthashabc123", 1, 2.0, None);
        std::fs::create_dir_all(cache.parent().unwrap()).expect("cache dir");
        std::fs::write(&cache, TINY_PNG).expect("write png");

        let first = read_cached_png(&cache).expect("first");
        assert_eq!(first.width, 1);
        assert_eq!(first.height, 1);
        let second = render_document_image(&state, "doc1", 1, None, Some(2.0)).expect("second");
        assert_eq!(second.png_base64, first.png_base64);
        assert_eq!(second.width, 1);
    }
}
