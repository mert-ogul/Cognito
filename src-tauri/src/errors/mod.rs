//! Application error types for Cognito desktop core.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CognitoError {
    #[error("database: {0}")]
    Database(String),
    #[error("sidecar spawn failed: program: {program}, detail: {detail}")]
    SidecarSpawn { program: String, detail: String },
    #[error("sidecar: {0}")]
    Sidecar(String),
    #[error("vault: {0}")]
    Vault(String),
    #[error("interrupted by chat")]
    InterruptedByChat,
}

pub type CognitoResult<T> = Result<T, CognitoError>;
