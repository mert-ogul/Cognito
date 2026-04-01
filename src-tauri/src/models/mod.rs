//! Shared DTOs for IPC and persistence boundaries.

mod payloads;
mod rpc_error;
mod rpc_request;
mod rpc_response;
pub use payloads::*;
pub use rpc_request::JsonRpcRequest;
pub use rpc_response::JsonRpcResponse;
