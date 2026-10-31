//! Compose [`AppHealth`](crate::models::AppHealth) from local probes and sidecar status.

use crate::db::sqlite;
use crate::services::checking_models::models_status_local;
use crate::errors::{CognitoError, CognitoResult};
use crate::models::AppHealth;
use crate::services::ollama_stream::ollama_base_url;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Configured model names that are not present in Ollama `/api/tags`.
/// Names without a tag match `"<name>:latest"`; tagged names match exactly.
pub(crate) fn missing_configured_models(
    configured: &[String],
    available: &[String],
) -> Vec<String> {
    configured
        .iter()
        .filter(|name| {
            let target = if name.contains(':') {
                (*name).clone()
            } else {
                format!("{name}:latest")
            };
            !available.iter().any(|t| t == &target)
        })
        .cloned()
        .collect()
}

fn configured_models_for_health(
    chat_model: &str,
    embed_model: &str,
    background_model: &str,
) -> Vec<String> {
    let mut models = vec![chat_model.to_string(), embed_model.to_string()];
    if background_model != chat_model {
        models.push(background_model.to_string());
    }
    models
}

async fn probe_ollama(
    chat_model: &str,
    embed_model: &str,
    background_model: &str,
) -> (bool, Vec<String>) {
    let configured = configured_models_for_health(chat_model, embed_model, background_model);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return (false, missing_configured_models(&configured, &[]));
        }
    };
    let url = format!("{}/api/tags", ollama_base_url().trim_end_matches('/'));
    let Ok(res) = client.get(&url).send().await else {
        return (false, missing_configured_models(&configured, &[]));
    };
    if !res.status().is_success() {
        return (false, missing_configured_models(&configured, &[]));
    }
    let Ok(body) = res.json::<Value>().await else {
        return (false, missing_configured_models(&configured, &[]));
    };
    let available: Vec<String> = body
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let missing = missing_configured_models(&configured, &available);
    (true, missing)
}

fn probe_checking_models(state: &crate::app::state::SharedState) -> bool {
    models_status_local(&state.data_dir).ready
}

pub async fn read_app_health(
    state: crate::app::state::SharedState,
) -> CognitoResult<AppHealth> {
    let st = state.clone();
    let sync = tauri::async_runtime::spawn_blocking(move || {
        let sqlite_ok = st.with_sqlite(|_| Ok(())).is_ok();
        let vault_configured = st.with_sqlite(|c| {
            let o = sqlite::get_setting(c, "vault_path")?;
            Ok(o.is_some())
        })?;
        let settings = st.with_sqlite(|c| sqlite::load_settings(c))?;
        let sidecar_reachable = st.sidecar.is_running();
        let warm_pending = st.chat_warm_ready().is_none();
        let chat_model_warmed = st.chat_warm_ready() == Some(true);
        let warm_up_error = st.chat_warm_error();
        let index_format_updating = st.index_format_updating.load(Ordering::SeqCst);
        let checking_models_ready = probe_checking_models(&st);
        Ok::<_, CognitoError>((
            sqlite_ok,
            vault_configured,
            settings,
            sidecar_reachable,
            warm_pending,
            chat_model_warmed,
            warm_up_error,
            index_format_updating,
            checking_models_ready,
        ))
    })
    .await
    .map_err(|e| CognitoError::Sidecar(e.to_string()))??;

    let (
        sqlite_ok,
        vault_configured,
        settings,
        sidecar_reachable,
        warm_pending,
        chat_model_warmed,
        warm_up_error,
        index_format_updating,
        checking_models_ready,
    ) = sync;

    let (ollama_reachable, missing_models) = probe_ollama(
        &settings.chat_model,
        &settings.embed_model,
        &settings.background_model,
    )
    .await;
    let ollama_model_ready = ollama_reachable && missing_models.is_empty();

    Ok(AppHealth {
        sqlite_ok,
        sidecar_reachable,
        ollama_model_ready,
        vault_configured,
        chat_model: settings.chat_model.clone(),
        embed_model: settings.embed_model,
        chat_model_warmed,
        warm_up_pending: warm_pending,
        warm_up_error,
        ollama_reachable,
        missing_models,
        index_format_updating,
        checking_models_ready,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_models_matches_latest_tag() {
        let available = vec![
            "qwen3.5:9b".into(),
            "qwen3-embedding:0.6b".into(),
        ];
        assert!(missing_configured_models(
            &["qwen3.5:9b".into(), "qwen3-embedding:0.6b".into()],
            &available
        )
        .is_empty());
        assert_eq!(
            missing_configured_models(&["qwen3.5:9b".into(), "other".into()], &available),
            vec!["other".to_string()]
        );
        assert_eq!(
            missing_configured_models(&["qwen3.5:9b-v2".into()], &available),
            vec!["qwen3.5:9b-v2".to_string()]
        );
        assert!(missing_configured_models(&["qwen3.5:9b".into()], &available).is_empty());
    }
}
