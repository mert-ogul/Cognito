//! Compose [`AppHealth`](crate::models::AppHealth) from database and sidecar probes.

use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::models::AppHealth;
use serde_json::json;

pub fn read_app_health(state: &crate::app::state::SharedState) -> CognitoResult<AppHealth> {
    let sqlite_ok = state.with_sqlite(|_| Ok(())).is_ok();
    let duckdb_ok = state.with_duckdb(|_| Ok(())).is_ok();
    let vault_configured = state.with_sqlite(|c| {
        let o = sqlite::get_setting(c, "vault_path")?;
        Ok(o.is_some())
    })?;
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    let mut sidecar_reachable = false;
    let mut ollama_model_ready = false;
    if let Ok(res) = state.sidecar.with_session(|s| {
        s.request(
            "health",
            json!({
                "chat_model": settings.chat_model,
                "embed_model": settings.embed_model,
            }),
        )
    }) {
        sidecar_reachable = true;
        ollama_model_ready = res
            .get("ollama_model_ready")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }
    let warm_pending = state.chat_warm_ready().is_none();
    let chat_model_warmed = state.chat_warm_ready() == Some(true);
    let warm_up_error = state.chat_warm_error();
    Ok(AppHealth {
        sqlite_ok,
        duckdb_ok,
        sidecar_reachable,
        ollama_model_ready,
        vault_configured,
        chat_model: settings.chat_model.clone(),
        embed_model: settings.embed_model,
        chat_model_warmed,
        warm_up_pending: warm_pending,
        warm_up_error,
    })
}
