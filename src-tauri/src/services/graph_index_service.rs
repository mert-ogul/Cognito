//! DuckDB graph reads for Tauri commands (2-hop, snapshot, evidence).

use crate::app::state::SharedState;
use crate::db::duckdb::graph_queries;
use crate::errors::CognitoResult;
use serde_json::Value;

pub fn search_graph_nodes(state: &SharedState, query: &str, limit: usize) -> CognitoResult<Vec<Value>> {
    state.with_duckdb(|c| graph_queries::search_nodes(c, query, limit))
}

pub fn get_graph_snapshot(state: &SharedState, limit_nodes: usize) -> CognitoResult<Value> {
    let edge_cap = (limit_nodes * 6).min(800);
    state.with_duckdb(|c| graph_queries::graph_snapshot(c, limit_nodes, edge_cap))
}

pub fn get_node_by_normalized_name(state: &SharedState, normalized: &str) -> CognitoResult<Option<Value>> {
    state.with_duckdb(|c| graph_queries::node_by_normalized_name(c, normalized))
}

pub fn get_neighbors_two_hop(state: &SharedState, node_id: &str) -> CognitoResult<Value> {
    state.with_duckdb(|c| graph_queries::neighbors_two_hop(c, node_id))
}

pub fn get_edges_for_document(state: &SharedState, document_id: &str) -> CognitoResult<Vec<Value>> {
    state.with_duckdb(|c| graph_queries::edges_for_document(c, document_id))
}

pub fn get_edges_for_chunk(state: &SharedState, chunk_id: &str) -> CognitoResult<Vec<Value>> {
    state.with_duckdb(|c| graph_queries::edges_for_chunk(c, chunk_id))
}

pub fn get_node_detail(state: &SharedState, node_id: &str) -> CognitoResult<Option<Value>> {
    state.with_duckdb(|c| graph_queries::node_detail(c, node_id))
}
