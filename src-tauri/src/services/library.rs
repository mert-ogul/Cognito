//! Library reset, vault switch, and forget.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::vault;
use serde_json::json;
use std::path::PathBuf;


pub fn reset(state: &SharedState) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    state.sidecar.with_session(|s| {
        s.request(
            "reset_index",
            json!({
                "sqlitePath": vault::path_string(&sqlite_path),
            }),
        )?;
        Ok(())
    })?;
    state.invalidate_sqlite()?;
    state.with_sqlite(|c| {
        sqlite::delete_all_documents(c)?;
        sqlite::clear_setting(c, "index_embed_model")?;
        Ok(())
    })?;
    Ok(())
}

/// Checks the folder, collects import paths (cap 100), resets if needed, then stores `vault_path`.
pub fn switch_vault(state: &SharedState, folder: &str) -> CognitoResult<Vec<PathBuf>> {
    let vault = vault::ensure_vault_path(folder)?;
    let paths = vault::collect_import_paths_bounded(&vault, vault::slots_remaining(0))?;
    let doc_n: i64 = state.with_sqlite(|c| {
        Ok(c.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?)
    })?;
    if doc_n > 0 {
        reset(state)?;
    }
    let stored = vault::path_string(&vault);
    state.with_sqlite(|c| sqlite::upsert_setting(c, "vault_path", &stored))?;
    Ok(paths)
}

pub fn forget(state: &SharedState) -> CognitoResult<()> {
    reset(state)?;
    state.with_sqlite(|c| sqlite::clear_setting(c, "vault_path"))?;
    Ok(())
}

/// Validates vault path and lists files for rebuild without mutating the library.
pub fn plan_rebuild(state: &SharedState) -> CognitoResult<(PathBuf, Vec<PathBuf>)> {
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    let vault_str = settings
        .vault_path
        .ok_or_else(|| CognitoError::Vault("Configure a vault folder first.".into()))?;
    let vault = vault::ensure_vault_path(&vault_str)?;
    let paths = vault::collect_import_paths_bounded(&vault, vault::slots_remaining(0))?;
    if paths.is_empty() {
        return Err(CognitoError::Vault("No PDF, Word or Markdown files found.".into()));
    }
    Ok((vault, paths))
}
