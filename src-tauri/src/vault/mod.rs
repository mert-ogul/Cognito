//! Library folder scanning and the 100-document first-import cap.

use crate::errors::{CognitoError, CognitoResult};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const MAX_FIRST_IMPORT: usize = 100;
pub const IMPORT_BATCH: usize = 20;

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Sorted PDF/Word/Markdown paths under `root`, at most `limit` files.
pub fn collect_import_paths_bounded(root: &Path, limit: usize) -> CognitoResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(64)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if is_importable_extension(p) {
            paths.push(p.to_path_buf());
        }
    }
    paths.sort();
    paths.truncate(limit);
    Ok(paths)
}

pub fn slots_remaining(active_doc_count: i64) -> usize {
    MAX_FIRST_IMPORT.saturating_sub(active_doc_count.max(0) as usize)
}

pub fn ensure_folder_in_vault(vault: &Path, folder: &Path) -> CognitoResult<PathBuf> {
    let v = vault
        .canonicalize()
        .map_err(|_| CognitoError::Vault("library folder path is invalid".into()))?;
    let f = folder
        .canonicalize()
        .map_err(|_| CognitoError::Vault("import path is invalid".into()))?;
    if !f.starts_with(&v) {
        return Err(CognitoError::Vault(
            "import folder must be inside the library folder".into(),
        ));
    }
    Ok(f)
}

fn is_pdf(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn is_markdown(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false)
}

fn is_docx(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("docx"))
        .unwrap_or(false)
}

pub fn is_importable_extension(p: &Path) -> bool {
    is_pdf(p) || is_markdown(p) || is_docx(p)
}

pub fn ensure_vault_path(raw: &str) -> CognitoResult<PathBuf> {
    let p = PathBuf::from(raw);
    if !p.exists() {
        return Err(CognitoError::Vault("That path does not exist.".into()));
    }
    if !p.is_dir() {
        return Err(CognitoError::Vault("That path is not a folder.".into()));
    }
    Ok(p)
}
