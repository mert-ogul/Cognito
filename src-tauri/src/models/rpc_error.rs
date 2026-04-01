//! JSON-RPC error object from the sidecar.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcErrorBody {
    pub code: i32,
    pub message: String,
}
