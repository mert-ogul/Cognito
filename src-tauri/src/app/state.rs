//! Shared application state for Tauri commands.

use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::sidecar::SidecarPool;
use rusqlite::Connection as SqlConn;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Background phi4-mini (chat model) warm-up status separate from Ollama daemon health.
pub type ChatWarmResult = Result<(), String>;

pub struct AppState {
    pub data_dir: PathBuf,
    sqlite_path: PathBuf,
    pub sidecar: SidecarPool,
    sql: Mutex<Option<SqlConn>>,
    pub watcher_tx: Mutex<Option<Sender<Option<PathBuf>>>>,
    pub background_tx: Mutex<Option<Sender<()>>>,
    /// `None` = warm-up not finished yet.
    pub chat_warm: Mutex<Option<ChatWarmResult>>,
    pub import_running: AtomicBool,
    /// True while a startup rebuild is rewriting embeddings for a new index format.
    pub index_format_updating: AtomicBool,
    /// Active `send_chat_query` tasks; background worker pauses while this is above zero.
    pub chat_in_flight: AtomicUsize,
}

impl AppState {
    pub fn new(data_dir: PathBuf, sidecar: SidecarPool) -> Self {
        let sqlite_path = data_dir.join("vault.sqlite");
        Self {
            data_dir,
            sqlite_path,
            sidecar,
            sql: Mutex::new(None),
            watcher_tx: Mutex::new(None),
            background_tx: Mutex::new(None),
            chat_warm: Mutex::new(None),
            import_running: AtomicBool::new(false),
            index_format_updating: AtomicBool::new(false),
            chat_in_flight: AtomicUsize::new(0),
        }
    }

    pub fn set_chat_warm(&self, result: ChatWarmResult) {
        if let Ok(mut g) = self.chat_warm.lock() {
            *g = Some(result);
        }
    }

    pub fn clear_chat_warm(&self) {
        if let Ok(mut g) = self.chat_warm.lock() {
            *g = None;
        }
    }

    pub fn chat_warm_ready(&self) -> Option<bool> {
        let g = self.chat_warm.lock().ok()?;
        match g.as_ref() {
            None => None,
            Some(Ok(())) => Some(true),
            Some(Err(_)) => Some(false),
        }
    }

    pub fn chat_warm_error(&self) -> Option<String> {
        let g = self.chat_warm.lock().ok()?;
        match g.as_ref() {
            Some(Err(e)) => Some(e.clone()),
            _ => None,
        }
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.sqlite_path.clone()
    }

    pub fn notify_watcher(&self, path: PathBuf) {
        if let Ok(g) = self.watcher_tx.lock() {
            if let Some(tx) = g.as_ref() {
                let _ = tx.send(Some(path));
            }
        }
    }

    pub fn stop_watcher(&self) {
        if let Ok(g) = self.watcher_tx.lock() {
            if let Some(tx) = g.as_ref() {
                let _ = tx.send(None);
            }
        }
    }

    pub fn notify_background_worker(&self) {
        if let Ok(g) = self.background_tx.lock() {
            if let Some(tx) = g.as_ref() {
                let _ = tx.send(());
            }
        }
    }

    pub fn with_sqlite<T, F>(&self, f: F) -> CognitoResult<T>
    where
        F: FnOnce(&SqlConn) -> CognitoResult<T>,
    {
        let mut g = self.sql.lock().map_err(|e| CognitoError::Database(e.to_string()))?;
        if g.is_none() {
            let c = sqlite::open_sqlite(&self.sqlite_path)?;
            *g = Some(c);
        }
        let conn = g.as_ref().ok_or_else(|| CognitoError::Database("sqlite".into()))?;
        f(conn)
    }

    /// Drop the cached connection so the next `with_sqlite` reopens after another process
    /// changed the schema (for example the sidecar dropping `chunk_embeddings`).
    pub fn invalidate_sqlite(&self) -> CognitoResult<()> {
        let mut g = self.sql.lock().map_err(|e| CognitoError::Database(e.to_string()))?;
        *g = None;
        Ok(())
    }
}

pub type SharedState = Arc<AppState>;
