//! Application error types for Cognito desktop core.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CognitoError {
    #[error("database: {0}")]
    Database(String),
    #[error("sidecar spawn failed — program: {program}, detail: {detail}")]
    SidecarSpawn { program: String, detail: String },
    #[error("sidecar: {0}")]
    Sidecar(String),
    #[error("vault: {0}")]
    Vault(String),
    #[error("import: {0}")]
    Import(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type CognitoResult<T> = Result<T, CognitoError>;
