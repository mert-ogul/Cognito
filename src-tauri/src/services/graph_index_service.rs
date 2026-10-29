//! Graph index reads via sidecar RPC (Python owns graph.duckdb).

use crate::app::state::SharedState;
use crate::errors::CognitoResult;
use serde_json::{json, Value};

fn sp(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn search_graph_nodes(
    state: &SharedState,
    query: &str,
    limit: usize,
) -> CognitoResult<Vec<Value>> {
    let duck = state.duckdb_path();
    let v = state.sidecar.with_session(|s| {
        s.request(
            "search_graph",
            json!({
                "duckdb_path": sp(&duck),
                "query": query,
                "limit": limit,
            }),
        )
    })?;
    Ok(v.get("nodes")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default())
}

pub fn get_graph_snapshot(state: &SharedState, limit_nodes: usize) -> CognitoResult<Value> {
    let duck = state.duckdb_path();
    state.sidecar.with_session(|s| {
        s.request(
            "get_graph_snapshot",
            json!({
                "duckdb_path": sp(&duck),
                "limit_nodes": limit_nodes,
            }),
        )
    })
}

pub fn get_node_detail(state: &SharedState, node_id: &str) -> CognitoResult<Option<Value>> {
    let duck = state.duckdb_path();
    let v = state.sidecar.with_session(|s| {
        s.request(
            "get_graph_node_detail",
            json!({
                "duckdb_path": sp(&duck),
                "node_id": node_id,
            }),
        )
    })?;
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(v))
}
