//! JSON-RPC request line (stdin/stdout sidecar protocol).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl JsonRpcRequest {
    pub fn ping(id: u64) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: "ping".into(),
            params: Value::Object(Default::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_serializes_round_trip() {
        let r = JsonRpcRequest::ping(7);
        let s = serde_json::to_string(&r).expect("ser");
        let back: JsonRpcRequest = serde_json::from_str(&s).expect("de");
        assert_eq!(back, r);
    }
}
