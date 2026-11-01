//! JSON-RPC sidecar over stdin/stdout (line-delimited JSON).

use crate::errors::{CognitoError, CognitoResult};
use crate::models::{JsonRpcRequest, JsonRpcResponse};
use crate::sidecar_paths::resolve_python_executable;
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static RPC_ID: AtomicU64 = AtomicU64::new(1);

const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub struct SidecarSession {
    child: Child,
    stdin: ChildStdin,
    line_rx: Receiver<String>,
}

fn timeout_for_method(method: &str) -> Duration {
    match method {
        "warm_model" => Duration::from_secs(600),
        "parse_documents" | "embed_chunks" => Duration::from_secs(1800),
        "prepare_chat_retrieval" | "prepare_related_retrieval" | "check_answer" => {
            Duration::from_secs(300)
        }
        _ => Duration::from_secs(120),
    }
}

fn prepare_log_file(log_path: &Path) -> CognitoResult<File> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CognitoError::Sidecar(e.to_string()))?;
    }
    if let Ok(meta) = std::fs::metadata(log_path) {
        if meta.len() > LOG_ROTATE_BYTES {
            let rotated = log_path
                .parent()
                .map(|p| p.join("sidecar.log.1"))
                .unwrap_or_else(|| PathBuf::from("sidecar.log.1"));
            let _ = std::fs::remove_file(&rotated);
            let _ = std::fs::rename(log_path, &rotated);
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| CognitoError::Sidecar(e.to_string()))
}

impl SidecarSession {
    pub fn spawn(
        main_py: &Path,
        cwd: &Path,
        log_path: &Path,
        app_data: &Path,
    ) -> CognitoResult<Self> {
        let py = resolve_python_executable(cwd);
        let program = py.to_string_lossy().into_owned();
        let log_file = prepare_log_file(log_path)?;
        let log = Arc::new(Mutex::new(log_file));
        let hf_home = app_data.join("models").join("hf");
        let _ = std::fs::create_dir_all(&hf_home);

        let mut child = Command::new(&py)
            .arg(main_py)
            .current_dir(cwd)
            .env("PYTHONNOUSERSITE", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUNBUFFERED", "1")
            .env("HF_HOME", &hf_home)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env_remove("PYTHONHOME")
            .env_remove("PYTHONPATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CognitoError::SidecarSpawn {
                program: program.clone(),
                detail: e.to_string(),
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CognitoError::Sidecar("stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CognitoError::Sidecar("stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CognitoError::Sidecar("stderr".into()))?;

        let log_err = log.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(mut f) = log_err.lock() {
                            let _ = write!(f, "{}", buf);
                            let _ = f.flush();
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let (tx, line_rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = buf.trim_end_matches(['\r', '\n']).to_string();
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            line_rx,
        })
    }

    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn kill_child(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn request(&mut self, method: &str, params: Value) -> CognitoResult<Value> {
        self.request_with_timeout(method, params, timeout_for_method(method))
    }

    pub fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> CognitoResult<Value> {
        let id = RPC_ID.fetch_add(1, Ordering::SeqCst);
        let req = if method == "ping" {
            JsonRpcRequest::ping(id)
        } else {
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id,
                method: method.to_string(),
                params,
            }
        };
        let line = serde_json::to_string(&req).map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        if writeln!(self.stdin, "{}", line).is_err() || self.stdin.flush().is_err() {
            self.kill_child();
            return Err(CognitoError::Sidecar("sidecar exited".into()));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.kill_child();
                return Err(CognitoError::Sidecar(format!(
                    "sidecar stopped responding to {} after {} s",
                    method,
                    timeout.as_secs()
                )));
            }
            match self.line_rx.recv_timeout(remaining) {
                Ok(raw) => {
                    if raw.trim().is_empty() {
                        continue;
                    }
                    let resp: JsonRpcResponse = match serde_json::from_str(raw.trim()) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("sidecar: ignoring unparseable line ({e}): {raw}");
                            continue;
                        }
                    };
                    if resp.id != id {
                        eprintln!(
                            "sidecar: ignoring response id {} (waiting for {})",
                            resp.id, id
                        );
                        continue;
                    }
                    if let Some(err) = resp.error {
                        return Err(CognitoError::Sidecar(format!(
                            "{}: {}",
                            err.code, err.message
                        )));
                    }
                    return Ok(resp.result.unwrap_or(Value::Null));
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.kill_child();
                    return Err(CognitoError::Sidecar(format!(
                        "sidecar stopped responding to {} after {} s",
                        method,
                        timeout.as_secs()
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.kill_child();
                    return Err(CognitoError::Sidecar("sidecar exited".into()));
                }
            }
        }
    }

    fn process_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

pub struct SidecarPool {
    main_py: PathBuf,
    cwd: PathBuf,
    log_path: PathBuf,
    app_data: PathBuf,
    inner: Arc<Mutex<Option<SidecarSession>>>,
}

impl SidecarPool {
    pub fn new(main_py: PathBuf, cwd: PathBuf, log_path: PathBuf, app_data: PathBuf) -> Self {
        Self {
            main_py,
            cwd,
            log_path,
            app_data,
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub fn main_py(&self) -> &Path {
        &self.main_py
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn ensure(&self) -> CognitoResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        let need_spawn = match g.as_mut() {
            None => true,
            Some(s) => !s.process_alive(),
        };
        if need_spawn {
            if let Some(mut old) = g.take() {
                old.shutdown();
            }
            *g = Some(SidecarSession::spawn(
                &self.main_py,
                &self.cwd,
                &self.log_path,
                &self.app_data,
            )?);
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if let Some(mut s) = g.take() {
            s.shutdown();
        }
    }

    /// True if a request holds the pool lock, or a live child process is present.
    pub fn is_running(&self) -> bool {
        match self.inner.try_lock() {
            Err(_) => true,
            Ok(mut g) => match g.as_mut() {
                None => false,
                Some(s) => s.process_alive(),
            },
        }
    }

    pub fn with_session<F, T>(&self, f: F) -> CognitoResult<T>
    where
        F: FnOnce(&mut SidecarSession) -> CognitoResult<T>,
    {
        self.ensure()?;
        let mut g = self
            .inner
            .lock()
            .map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        let session = g
            .as_mut()
            .ok_or_else(|| CognitoError::Sidecar("no session".into()))?;
        let result = f(session);
        if !session.process_alive() {
            if let Some(mut dead) = g.take() {
                dead.shutdown();
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn write_tiny_sidecar(dir: &Path) -> PathBuf {
        let main_py = dir.join("main.py");
        fs::write(
            &main_py,
            r#"
import json, sys, time
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    method = req.get("method")
    rid = req.get("id")
    if method == "crash":
        sys.exit(1)
    if method == "hang":
        time.sleep(9999)
        continue
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": rid, "result": {"pong": True}}) + "\n")
    sys.stdout.flush()
"#,
        )
        .expect("write tiny sidecar");
        main_py
    }

    #[test]
    fn sidecar_recovers_from_crash_and_hang() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let main_py = write_tiny_sidecar(dir.path());
        let log = dir.path().join("sidecar.log");
        let pool = SidecarPool::new(
            main_py,
            dir.path().to_path_buf(),
            log,
            dir.path().to_path_buf(),
        );

        pool.with_session(|s| s.request("ping", json!({})))
            .expect("initial ping");

        let crash = pool.with_session(|s| s.request("crash", json!({})));
        assert!(crash.is_err(), "crash should err");

        pool.with_session(|s| s.request("ping", json!({})))
            .expect("ping after crash");

        let hang = pool
            .with_session(|s| s.request_with_timeout("hang", json!({}), Duration::from_secs(1)));
        assert!(hang.is_err(), "hang should timeout");

        pool.with_session(|s| s.request("ping", json!({})))
            .expect("ping after hang");
    }
}
