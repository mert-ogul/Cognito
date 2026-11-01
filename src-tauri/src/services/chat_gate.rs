//! Wait while a chat query holds the sidecar / GPU.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::services::ollama_stream;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

/// Cap so a stuck chat cannot stall an import forever.
pub const IMPORT_CHAT_WAIT_CAP: Duration = Duration::from_secs(180);

pub fn chat_in_flight(state: &SharedState) -> bool {
    state.chat_in_flight.load(Ordering::SeqCst) > 0
}

/// Block while `chat_in_flight > 0`, up to `cap`. Returns whether chat is idle
/// (`false` means the cap was hit while a chat was still running).
pub fn wait_until_chat_idle(state: &SharedState, cap: Duration) -> bool {
    let start = Instant::now();
    while chat_in_flight(state) {
        if start.elapsed() >= cap {
            eprintln!(
                "chat wait cap {:?} hit while chat_in_flight > 0; continuing",
                cap
            );
            return false;
        }
        thread::sleep(Duration::from_millis(500));
    }
    true
}

pub enum WaitKind {
    Entities,
    Card,
}

fn unload_background(state: &SharedState) {
    let settings = match state.with_sqlite(sqlite::load_settings) {
        Ok(s) => s,
        Err(_) => return,
    };
    let background = settings.background_model.trim();
    let chat = settings.chat_model.trim();
    if background.is_empty() || background == chat {
        return;
    }
    ollama_stream::unload_model_blocking(background);
}

/// Used by the background worker: wait for chat, unload background model once,
/// and abort if the document is no longer running / feature disabled.
pub fn wait_while_chat_in_flight(
    state: &SharedState,
    document_id: &str,
    kind: WaitKind,
) -> CognitoResult<bool> {
    let mut unloaded = false;
    while chat_in_flight(state) {
        if !unloaded {
            unload_background(state);
            unloaded = true;
        }
        match kind {
            WaitKind::Card => {
                let still =
                    state.with_sqlite(|c| sqlite::document_card_still_running(c, document_id))?;
                if !still {
                    return Ok(false);
                }
                let enabled = state
                    .with_sqlite(sqlite::load_settings)?
                    .summarise_documents;
                if !enabled {
                    state.with_sqlite(|c| sqlite::mark_document_card_pending(c, document_id))?;
                    return Ok(false);
                }
            }
            WaitKind::Entities => {
                let still = state
                    .with_sqlite(|c| sqlite::document_entities_still_running(c, document_id))?;
                if !still {
                    return Ok(false);
                }
                let enabled = state.with_sqlite(sqlite::load_settings)?.extract_entities;
                if !enabled {
                    state
                        .with_sqlite(|c| sqlite::mark_document_entities_pending(c, document_id))?;
                    return Ok(false);
                }
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(true)
}
