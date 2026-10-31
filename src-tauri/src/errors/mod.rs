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

impl From<rusqlite::Error> for CognitoError {
    fn from(value: rusqlite::Error) -> Self {
        CognitoError::Database(value.to_string())
    }
}

pub type CognitoResult<T> = Result<T, CognitoError>;
