//! Resolve Python sidecar `main.py` and working directory for development vs packaged apps.
//!
//! Priority:
//! 1. `COGNITO_SIDECAR_ROOT` — absolute path to sidecar directory (tests / custom layouts).
//! 2. Bundled resources — `resource_dir()/sidecar` when `main.py` exists (production install).
//! 3. Development — `CARGO_MANIFEST_DIR/../sidecar` (crate adjacent to repo `sidecar/`).

use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

/// Returns `(main_py, cwd)` where `cwd` is the sidecar root (contains `main.py`, `.venv`, etc.).
pub fn resolve_sidecar_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    if let Ok(root) = std::env::var("COGNITO_SIDECAR_ROOT") {
        let cwd = PathBuf::from(root);
        let main_py = cwd.join("main.py");
        if main_py.is_file() {
            return Ok((main_py, cwd));
        }
        return Err(format!(
            "COGNITO_SIDECAR_ROOT set but main.py missing: {}",
            main_py.display()
        ));
    }

    if let Ok(res) = app.path().resource_dir() {
        let cwd = res.join("sidecar");
        let main_py = cwd.join("main.py");
        if main_py.is_file() {
            return Ok((main_py, cwd));
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cwd = manifest.join("../sidecar");
    let main_py = cwd.join("main.py");
    Ok((main_py, cwd))
}

/// Python interpreter inside the sidecar venv, resolved relative to sidecar `cwd`.
pub fn resolve_python_executable(cwd: &std::path::Path) -> PathBuf {
    if let Ok(p) = std::env::var("COGNITO_PYTHON") {
        return PathBuf::from(p);
    }
    #[cfg(unix)]
    {
        let py3 = cwd.join(".venv/bin/python3");
        if py3.is_file() {
            return py3;
        }
        let py = cwd.join(".venv/bin/python");
        if py.is_file() {
            return py;
        }
        return PathBuf::from("python3");
    }
    #[cfg(windows)]
    {
        let win = cwd.join(".venv/Scripts/python.exe");
        if win.is_file() {
            return win;
        }
        return PathBuf::from("python");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_python_falls_back_name() {
        let cwd = std::path::PathBuf::from("/nonexistent/sidecar");
        let p = resolve_python_executable(&cwd);
        assert!(!p.as_os_str().is_empty());
    }
}
