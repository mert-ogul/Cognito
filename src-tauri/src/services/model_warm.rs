//! Pre-load the configured chat model via sidecar `warm_model` on startup.

use crate::app::state::SharedState;
use crate::db::sqlite;
use serde_json::json;
use std::time::Duration;

pub fn spawn_warm_chat_model(state: SharedState) {
    std::thread::spawn(move || {
        let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
            Ok(s) => s,
            Err(e) => {
                state.set_chat_warm(Err(format!(
                    "Could not load settings: {}. Fix app data and restart.",
                    e
                )));
                return;
            }
        };
        let r = state.sidecar.with_session(|s| {
            s.request(
                "warm_model",
                json!({ "chat_model": settings.chat_model }),
            )
        });
        match r {
            Ok(_) => {
                state.set_chat_warm(Ok(()));
            }
            Err(e) => {
                let msg = format!(
                    "Model warm-up failed: {}. Start Ollama, run `ollama pull {}`, then click Retry.",
                    e, settings.chat_model
                );
                state.set_chat_warm(Err(msg));
            }
        }
    });
}

/// After chat warm, load MiniLM + HHEM on the sidecar's first idle moment.
pub fn spawn_warm_checking_models(state: SharedState) {
    std::thread::spawn(move || {
        // Let the chat-model warm and UI settle first.
        std::thread::sleep(Duration::from_secs(2));
        let thorough = state
            .with_sqlite(|c| sqlite::load_settings(c))
            .map(|s| s.thorough_reranking)
            .unwrap_or(false);
        let app_data = state.data_dir.to_string_lossy().into_owned();
        let status = state.sidecar.with_session(|s| {
            s.request(
                "models_status",
                json!({ "app_data": app_data.clone(), "thorough_reranking": thorough }),
            )
        });
        let ready = status
            .ok()
            .and_then(|v| v.get("ready").and_then(|x| x.as_bool()))
            .unwrap_or(false);
        if !ready {
            return;
        }
        let _ = state.sidecar.with_session(|s| {
            s.request(
                "warm_checking_models",
                json!({ "app_data": app_data, "thorough_reranking": thorough }),
            )
        });
    });
}
