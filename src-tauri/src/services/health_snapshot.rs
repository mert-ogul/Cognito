//! Compose [`AppHealth`](crate::models::AppHealth) from local probes and sidecar status.

use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::models::AppHealth;
use crate::services::ollama_stream::ollama_base_url;
use serde_json::{json, Value};
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

async fn probe_ollama(chat_model: &str, embed_model: &str) -> (bool, Vec<String>) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return (
                false,
                missing_configured_models(
                    &[chat_model.to_string(), embed_model.to_string()],
                    &[],
                ),
            );
        }
    };
    let url = format!("{}/api/tags", ollama_base_url().trim_end_matches('/'));
    let Ok(res) = client.get(&url).send().await else {
        return (
            false,
            missing_configured_models(&[chat_model.to_string(), embed_model.to_string()], &[]),
        );
    };
    if !res.status().is_success() {
        return (
            false,
            missing_configured_models(&[chat_model.to_string(), embed_model.to_string()], &[]),
        );
    }
    let Ok(body) = res.json::<Value>().await else {
        return (
            false,
            missing_configured_models(&[chat_model.to_string(), embed_model.to_string()], &[]),
        );
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
    let missing = missing_configured_models(
        &[chat_model.to_string(), embed_model.to_string()],
        &available,
    );
    (true, missing)
}

fn probe_checking_models(state: &crate::app::state::SharedState) -> bool {
    if std::env::var("COGNITO_MOCK_MODELS").ok().as_deref() == Some("1") {
        return true;
    }
    let app_data = state.data_dir.to_string_lossy().into_owned();
    let thorough = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map(|s| s.thorough_reranking)
        .unwrap_or(false);
    match state.sidecar.with_session(|s| {
        s.request(
            "models_status",
            json!({ "app_data": app_data, "thorough_reranking": thorough }),
        )
    }) {
        Ok(v) => v.get("ready").and_then(|x| x.as_bool()).unwrap_or(false),
        Err(_) => false,
    }
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

    let (ollama_reachable, missing_models) =
        probe_ollama(&settings.chat_model, &settings.embed_model).await;
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
        let available = vec!["phi4-mini:latest".into(), "nomic-embed-text:latest".into()];
        assert!(missing_configured_models(
            &["phi4-mini".into(), "nomic-embed-text".into()],
            &available
        )
        .is_empty());
        assert_eq!(
            missing_configured_models(&["phi4-mini".into(), "other".into()], &available),
            vec!["other".to_string()]
        );
        assert_eq!(
            missing_configured_models(&["phi4-mini:v2".into()], &available),
            vec!["phi4-mini:v2".to_string()]
        );
        assert!(missing_configured_models(
            &["phi4-mini:latest".into()],
            &available
        )
        .is_empty());
    }
}
