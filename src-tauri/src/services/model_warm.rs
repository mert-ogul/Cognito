//! Pre-load the configured chat model via sidecar `warm_model` on startup.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::MODEL_WARM_PROGRESS;
use serde_json::json;
use tauri::{AppHandle, Emitter};

pub fn spawn_warm_chat_model(state: SharedState, app: AppHandle) {
    std::thread::spawn(move || {
        let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
            Ok(s) => s,
            Err(e) => {
                state.set_chat_warm(Err(format!(
                    "Could not load settings: {}. Fix app data and restart.",
                    e
                )));
                let _ = app.emit(
                    MODEL_WARM_PROGRESS,
                    &json!({ "ok": false, "error": e.to_string() }),
                );
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
                let _ = app.emit(MODEL_WARM_PROGRESS, &json!({ "ok": true }));
            }
            Err(e) => {
                let msg = format!(
                    "Model warm-up failed: {}. Start Ollama, run `ollama pull {}`, then restart Cognito.",
                    e, settings.chat_model
                );
                state.set_chat_warm(Err(msg.clone()));
                let _ = app.emit(
                    MODEL_WARM_PROGRESS,
                    &json!({ "ok": false, "error": e.to_string(), "hint": msg }),
                );
            }
        }
    });
}
