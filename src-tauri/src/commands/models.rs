use crate::app::state::SharedState;
use crate::events::MODELS_PROGRESS;
use crate::models::{ModelsProgressEvent, ModelsStatus};
use crate::services::checking_models::{dir_size_bytes, models_root, models_status_local};
use crate::sidecar_paths::resolve_python_executable;
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

fn app_data_string(state: &SharedState) -> String {
    state.data_dir.to_string_lossy().into_owned()
}

#[tauri::command]
pub async fn prepare_models(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<ModelsStatus, String> {
    let st = state.inner().clone();
    let app_c = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app_data = app_data_string(&st);
        let app_data_path = PathBuf::from(&app_data);
        let cwd = st.sidecar.cwd().to_path_buf();
        let main_py = st.sidecar.main_py().to_path_buf();
        let py = resolve_python_executable(&cwd);
        let hf_home = models_root(&app_data_path).join("hf");
        std::fs::create_dir_all(models_root(&app_data_path)).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&hf_home).map_err(|e| e.to_string())?;

        let mut child = Command::new(&py)
            .arg(&main_py)
            .arg("download-models")
            .arg("--app-data")
            .arg(&app_data)
            .current_dir(&cwd)
            .env("HF_HOME", &hf_home)
            .env("PYTHONNOUSERSITE", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUNBUFFERED", "1")
            .env_remove("PYTHONHOME")
            .env_remove("PYTHONPATH")
            .env_remove("HF_HUB_OFFLINE")
            .env_remove("TRANSFORMERS_OFFLINE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;

        let total_bytes = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let total_for_reader = total_bytes.clone();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "download stdout".to_string())?;
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if let Some(n) = v.get("totalBytes").and_then(|x| x.as_u64()) {
                    total_for_reader.store(n, Ordering::SeqCst);
                }
            }
        });
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for _line in reader.lines() {}
            });
        }

        let models_path = models_root(&app_data_path);
        let total_for_emit = total_bytes.clone();
        let stop_emit = stop.clone();
        let app_emit = app_c.clone();
        thread::spawn(move || {
            while !stop_emit.load(Ordering::SeqCst) {
                let downloaded = dir_size_bytes(&models_path);
                let total = total_for_emit.load(Ordering::SeqCst);
                let _ = app_emit.emit(
                    MODELS_PROGRESS,
                    &ModelsProgressEvent {
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                    },
                );
                thread::sleep(Duration::from_millis(500));
            }
        });

        let status = child.wait().map_err(|e| e.to_string())?;
        stop.store(true, Ordering::SeqCst);
        if !status.success() {
            return Err(format!("download-models exited with {status}"));
        }

        let downloaded = dir_size_bytes(&models_root(&app_data_path));
        let total = total_bytes.load(Ordering::SeqCst);
        let _ = app_c.emit(
            MODELS_PROGRESS,
            &ModelsProgressEvent {
                downloaded_bytes: downloaded,
                total_bytes: total,
            },
        );

        let _ = st.sidecar.with_session(|s| {
            s.request(
                "warm_checking_models",
                json!({ "app_data": app_data_string(&st) }),
            )
        });
        Ok(models_status_local(&st.data_dir))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_models_status(
    state: tauri::State<'_, SharedState>,
) -> Result<ModelsStatus, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(models_status_local(&st.data_dir)))
        .await
        .map_err(|e| e.to_string())?
}
