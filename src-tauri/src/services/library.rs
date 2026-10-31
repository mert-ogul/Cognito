//! Library reset, vault switch, and forget.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::vault;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn clear_page_cache(data_dir: &std::path::Path) -> CognitoResult<()> {
    let dir = data_dir.join("page_cache");
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir).map_err(|e| CognitoError::Vault(e.to_string()))? {
        let entry = entry.map_err(|e| CognitoError::Vault(e.to_string()))?;
        let path = entry.path();
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

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
        sqlite::empty_connection_tables(c)?;
        sqlite::conversations::delete_all_conversations(c)?;
        sqlite::clear_setting(c, "index_embed_model")?;
        Ok(())
    })?;
    clear_page_cache(&state.data_dir)?;
    Ok(())
}

/// Checks the folder, collects import paths (cap 100), resets if needed, then stores `vault_path`.
pub fn switch_vault(state: &SharedState, folder: &str) -> CognitoResult<Vec<PathBuf>> {
    let vault = vault::ensure_vault_path(folder)?;
    let paths = vault::collect_import_paths_bounded(&vault, vault::slots_remaining(0))?;
    let doc_n: i64 = state
        .with_sqlite(|c| Ok(c.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?))?;
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
    let settings = state.with_sqlite(sqlite::load_settings)?;
    let vault_str = settings
        .vault_path
        .ok_or_else(|| CognitoError::Vault("Choose a library folder first.".into()))?;
    let vault = vault::ensure_vault_path(&vault_str)?;
    let paths = vault::collect_import_paths_bounded(&vault, vault::slots_remaining(0))?;
    if paths.is_empty() {
        return Err(CognitoError::Vault(
            "No PDF, Word or Markdown files found.".into(),
        ));
    }
    Ok((vault, paths))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::sidecar::SidecarPool;
    use std::fs;
    use std::sync::Arc;

    #[test]
    fn reset_empties_entities_and_page_cache() {
        let data = tempfile::tempdir().expect("data");
        let sidecar_cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.path().join("logs").join("sidecar.log");
        fs::create_dir_all(log_path.parent().unwrap()).expect("logs");
        let pool = SidecarPool::new(main_py, sidecar_cwd, log_path, data.path().to_path_buf());
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        let _ = state.sidecar.ensure();

        let cache = data.path().join("page_cache");
        fs::create_dir_all(&cache).expect("cache");
        fs::write(cache.join("doc-hash-p1-s200.png"), b"png").expect("png");

        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c, "d1", "/a.md", "A", "markdown", "h", None, "indexed", None, None,
                )?;
                sqlite::insert_chunk_row(
                    c, "c1", "d1", 0, None, None, "Ada", 1, "h1", "body", "[]", None,
                )?;
                c.execute(
                    "INSERT INTO entities(id, label, name, normalized, doc_count, mention_count)
                     VALUES ('e1', 'person', 'Ada', 'ada', 1, 1)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO mentions(entity_id, chunk_id, document_id, start, end)
                     VALUES ('e1', 'c1', 'd1', 0, 3)",
                    [],
                )?;
                Ok(())
            })
            .expect("seed");

        reset(&state).expect("reset");

        state
            .with_sqlite(|c| {
                let ents: i64 = c.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
                assert_eq!(ents, 0);
                let docs: i64 = c.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
                assert_eq!(docs, 0);
                Ok(())
            })
            .expect("assert db");

        let left: Vec<_> = fs::read_dir(&cache)
            .expect("read cache")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();
        assert!(left.is_empty(), "page_cache should be empty");
    }

    #[test]
    fn switch_vault_and_forget_remove_conversations() {
        let data = tempfile::tempdir().expect("data");
        let sidecar_cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.path().join("logs").join("sidecar.log");
        fs::create_dir_all(log_path.parent().unwrap()).expect("logs");
        let pool = SidecarPool::new(main_py, sidecar_cwd, log_path, data.path().to_path_buf());
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        let _ = state.sidecar.ensure();

        let vault_a = data.path().join("vault_a");
        fs::create_dir_all(&vault_a).expect("vault_a");
        fs::write(vault_a.join("a.md"), "# A\n").expect("a.md");

        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "vault_path", &vault::path_string(&vault_a))?;
                sqlite::insert_document_row(
                    c,
                    "d1",
                    &vault::path_string(&vault_a.join("a.md")),
                    "A",
                    "markdown",
                    "h",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                let conv = sqlite::conversations::create_conversation(c, "Keep me?")?;
                sqlite::conversations::append_turn(c, &conv.id, "u1", "Keep me?", "m1")?;
                Ok(())
            })
            .expect("seed");

        let vault_b = data.path().join("vault_b");
        fs::create_dir_all(&vault_b).expect("vault_b");
        fs::write(vault_b.join("b.md"), "# B\n").expect("b.md");
        switch_vault(&state, &vault::path_string(&vault_b)).expect("switch");
        state
            .with_sqlite(|c| {
                let n = sqlite::conversations::list_conversations(c)?.len();
                assert_eq!(n, 0);
                Ok(())
            })
            .expect("after switch");

        state
            .with_sqlite(|c| {
                let _ = sqlite::conversations::create_conversation(c, "Again")?;
                Ok(())
            })
            .expect("reseed");
        forget(&state).expect("forget");
        state
            .with_sqlite(|c| {
                let n = sqlite::conversations::list_conversations(c)?.len();
                assert_eq!(n, 0);
                Ok(())
            })
            .expect("after forget");
    }
}
