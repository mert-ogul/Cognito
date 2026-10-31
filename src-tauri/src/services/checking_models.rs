//! Local presence checks for checking models (`.complete` markers under app data).

use crate::models::{ModelPresence, ModelsStatus};
use std::path::{Path, PathBuf};

const LOCAL_REPOS: &[(&str, &str)] = &[
    (
        "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1",
        "cross-encoder--mmarco-mMiniLMv2-L12-H384-v1",
    ),
    (
        "vectara/hallucination_evaluation_model",
        "vectara--hallucination_evaluation_model",
    ),
    ("urchade/gliner_multi-v2.1", "urchade--gliner_multi-v2.1"),
];

pub fn models_root(app_data: &Path) -> PathBuf {
    app_data.join("models")
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(meta) = std::fs::metadata(&p) {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

pub fn models_status_local(app_data: &Path) -> ModelsStatus {
    if std::env::var("COGNITO_MOCK_MODELS").ok().as_deref() == Some("1") {
        return ModelsStatus {
            ready: true,
            models: LOCAL_REPOS
                .iter()
                .map(|(name, _)| ModelPresence {
                    name: (*name).to_string(),
                    present: true,
                    size_bytes: 0,
                })
                .collect(),
            total_size_bytes: 0,
        };
    }
    let root = models_root(app_data);
    let mut models = Vec::with_capacity(LOCAL_REPOS.len());
    let mut total = 0u64;
    let mut ready = true;
    for (name, folder) in LOCAL_REPOS {
        let path = root.join(folder);
        let present = path.join(".complete").is_file();
        if !present {
            ready = false;
        }
        let size = if present { dir_size_bytes(&path) } else { 0 };
        total = total.saturating_add(size);
        models.push(ModelPresence {
            name: (*name).to_string(),
            present,
            size_bytes: size,
        });
    }
    ModelsStatus {
        ready,
        models,
        total_size_bytes: total,
    }
}

pub fn is_missing_model_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("not downloaded") || lower.contains("missing .complete")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_complete_means_absent() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let folder = dir
            .path()
            .join("models")
            .join("cross-encoder--mmarco-mMiniLMv2-L12-H384-v1");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("config.json"), b"{}").unwrap();
        let status = models_status_local(dir.path());
        assert!(!status.ready);
        let mini = status
            .models
            .iter()
            .find(|m| m.name.contains("mMiniLM"))
            .expect("minilm");
        assert!(!mini.present);
        fs::write(folder.join(".complete"), b"").unwrap();
        // Still not ready until all three are complete.
        let status2 = models_status_local(dir.path());
        let mini2 = status2
            .models
            .iter()
            .find(|m| m.name.contains("mMiniLM"))
            .expect("minilm");
        assert!(mini2.present);
        assert!(!status2.ready);
    }
}
