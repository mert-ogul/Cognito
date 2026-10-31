//! Resolve Python sidecar `main.py` and working directory for development vs packaged apps.
//!
//! Priority:
//! 1. `COGNITO_SIDECAR_ROOT` — absolute path to sidecar directory (tests / custom layouts).
//! 2. Bundled resources — `resource_dir()/sidecar` when `main.py` exists (production install).
//! 3. Development — `CARGO_MANIFEST_DIR/../sidecar` (crate adjacent to repo `sidecar/`).

use std::path::{Path, PathBuf};
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

fn bundled_python(cwd: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let bin = cwd.join("python/bin");
        let py3 = bin.join("python3");
        if py3.is_file() {
            return Some(py3);
        }
        // Symlink may be missing or broken; prefer a concrete python3.N next to it.
        if let Ok(entries) = std::fs::read_dir(&bin) {
            let mut candidates: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    name.starts_with("python3.")
                        && !name.contains('-')
                        && !name.contains("config")
                        && p.is_file()
                })
                .collect();
            candidates.sort();
            if let Some(p) = candidates.into_iter().next() {
                return Some(p);
            }
        }
        None
    }
    #[cfg(windows)]
    {
        let win = cwd.join("python/python.exe");
        if win.is_file() {
            Some(win)
        } else {
            None
        }
    }
}

fn venv_python(cwd: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let py3 = cwd.join(".venv/bin/python3");
        if py3.is_file() {
            return Some(py3);
        }
        let py = cwd.join(".venv/bin/python");
        if py.is_file() {
            return Some(py);
        }
        None
    }
    #[cfg(windows)]
    {
        let win = cwd.join(".venv/Scripts/python.exe");
        if win.is_file() {
            Some(win)
        } else {
            None
        }
    }
}

/// Python interpreter for the sidecar, resolved relative to sidecar `cwd`.
///
/// Order: `COGNITO_PYTHON` → bundled standalone CPython → `.venv` (dev) → `python3`/`python`.
pub fn resolve_python_executable(cwd: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("COGNITO_PYTHON") {
        return PathBuf::from(p);
    }
    if let Some(p) = bundled_python(cwd) {
        return p;
    }
    if let Some(p) = venv_python(cwd) {
        return p;
    }
    #[cfg(unix)]
    {
        PathBuf::from("python3")
    }
    #[cfg(windows)]
    {
        PathBuf::from("python")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_python_falls_back_name() {
        let cwd = PathBuf::from("/nonexistent/sidecar");
        // Avoid inheriting a host override in CI/dev shells.
        let _guard = EnvGuard::clear("COGNITO_PYTHON");
        let p = resolve_python_executable(&cwd);
        assert!(!p.as_os_str().is_empty());
        #[cfg(unix)]
        assert_eq!(p, PathBuf::from("python3"));
        #[cfg(windows)]
        assert_eq!(p, PathBuf::from("python"));
    }

    #[test]
    fn resolve_python_prefers_bundled_then_venv() {
        let _guard = EnvGuard::clear("COGNITO_PYTHON");
        let dir = tempfile::tempdir().expect("tmpdir");
        let cwd = dir.path();

        #[cfg(unix)]
        {
            let venv_bin = cwd.join(".venv/bin");
            fs::create_dir_all(&venv_bin).unwrap();
            write_exec(&venv_bin.join("python3"));

            let without_bundle = resolve_python_executable(cwd);
            assert_eq!(without_bundle, venv_bin.join("python3"));

            let bin = cwd.join("python/bin");
            fs::create_dir_all(&bin).unwrap();
            write_exec(&bin.join("python3"));
            let with_bundle = resolve_python_executable(cwd);
            assert_eq!(with_bundle, bin.join("python3"));
        }
        #[cfg(windows)]
        {
            let venv = cwd.join(".venv/Scripts");
            fs::create_dir_all(&venv).unwrap();
            fs::write(venv.join("python.exe"), b"").unwrap();
            assert_eq!(resolve_python_executable(cwd), venv.join("python.exe"));

            let py_dir = cwd.join("python");
            fs::create_dir_all(&py_dir).unwrap();
            fs::write(py_dir.join("python.exe"), b"").unwrap();
            assert_eq!(resolve_python_executable(cwd), py_dir.join("python.exe"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_python_uses_versioned_binary_when_python3_missing() {
        let _guard = EnvGuard::clear("COGNITO_PYTHON");
        let dir = tempfile::tempdir().expect("tmpdir");
        let bin = dir.path().join("python/bin");
        fs::create_dir_all(&bin).unwrap();
        write_exec(&bin.join("python3.12"));
        let p = resolve_python_executable(dir.path());
        assert_eq!(p, bin.join("python3.12"));
    }

    #[cfg(unix)]
    fn write_exec(path: &Path) {
        fs::write(path, b"").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    /// Restores a previous env var value when dropped.
    struct EnvGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn clear(key: &'static str) -> Self {
            let prev = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
