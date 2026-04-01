//! JSON-RPC sidecar over stdin/stdout (line-delimited JSON).

use crate::errors::{CognitoError, CognitoResult};
use crate::sidecar_paths::resolve_python_executable;
use crate::models::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static RPC_ID: AtomicU64 = AtomicU64::new(1);

pub struct SidecarSession {
    pub child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn drain_stderr(mut stderr: ChildStderr) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            if stderr.read(&mut buf).unwrap_or(0) == 0 {
                break;
            }
        }
    });
}

impl SidecarSession {
    pub fn spawn(main_py: &Path, cwd: &Path) -> CognitoResult<Self> {
        let py = resolve_python_executable(cwd);
        let program = py.to_string_lossy().into_owned();
        let mut child = Command::new(&py)
            .arg(main_py)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CognitoError::SidecarSpawn {
                program: program.clone(),
                detail: e.to_string(),
            })?;
        let stdin = child.stdin.take().ok_or_else(|| CognitoError::Sidecar("stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| CognitoError::Sidecar("stdout".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| CognitoError::Sidecar("stderr".into()))?;
        drain_stderr(stderr);
        let stdout = BufReader::new(stdout);
        Ok(Self { child, stdin, stdout })
    }

    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn request(&mut self, method: &str, params: Value) -> CognitoResult<Value> {
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
        writeln!(self.stdin, "{}", line).map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        self.stdin.flush().map_err(|e| CognitoError::Sidecar(e.to_string()))?;

        let mut buf = String::new();
        self.stdout
            .read_line(&mut buf)
            .map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        let resp: JsonRpcResponse = serde_json::from_str(buf.trim()).map_err(|e| {
            CognitoError::Sidecar(format!("parse: {} body: {}", e, buf))
        })?;
        if let Some(err) = resp.error {
            return Err(CognitoError::Sidecar(format!("{}: {}", err.code, err.message)));
        }
        resp.result
            .ok_or_else(|| CognitoError::Sidecar("missing result".into()))
    }
}

pub struct SidecarPool {
    main_py: PathBuf,
    cwd: PathBuf,
    inner: Arc<Mutex<Option<SidecarSession>>>,
}

impl SidecarPool {
    pub fn new(main_py: PathBuf, cwd: PathBuf) -> Self {
        Self {
            main_py,
            cwd,
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub fn ensure(&self) -> CognitoResult<()> {
        let mut g = self.inner.lock().map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        if g.is_none() {
            let s = SidecarSession::spawn(&self.main_py, &self.cwd)?;
            *g = Some(s);
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

    pub fn with_session<F, T>(&self, f: F) -> CognitoResult<T>
    where
        F: FnOnce(&mut SidecarSession) -> CognitoResult<T>,
    {
        self.ensure()?;
        let mut g = self.inner.lock().map_err(|e| CognitoError::Sidecar(e.to_string()))?;
        let session = g
            .as_mut()
            .ok_or_else(|| CognitoError::Sidecar("no session".into()))?;
        f(session)
    }
}
