//! Drop embedding index + graph rows and clear library document rows.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use serde_json::json;
use std::path::Path;

fn sp(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn reset_library(state: &SharedState) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let duck_path = state.duckdb_path();
    state.sidecar.with_session(|s| {
        s.request(
            "reset_index",
            json!({
                "sqlitePath": sp(&sqlite_path),
                "duckPath": sp(&duck_path),
            }),
        )?;
        Ok(())
    })?;
    state.with_sqlite(|c| {
        sqlite::delete_all_documents(c)?;
        sqlite::clear_setting(c, "index_embed_model")?;
        Ok(())
    })?;
    Ok(())
}
