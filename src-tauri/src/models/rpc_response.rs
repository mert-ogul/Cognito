//! JSON-RPC response line from the sidecar.

use super::rpc_error::JsonRpcErrorBody;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcErrorBody>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_payload() {
        let r = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(serde_json::json!({ "pong": true })),
            error: None,
        };
        let s = serde_json::to_string(&r).expect("ser");
        let back: JsonRpcResponse = serde_json::from_str(&s).expect("de");
        assert_eq!(back, r);
    }
}
