use crate::app::state::SharedState;
use crate::services::graph_index_service;
use serde_json::Value;

#[tauri::command]
pub fn search_graph(
    state: tauri::State<'_, SharedState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    graph_index_service::search_graph_nodes(&state, &query, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_graph_snapshot(
    state: tauri::State<'_, SharedState>,
    limit_nodes: Option<usize>,
) -> Result<Value, String> {
    graph_index_service::get_graph_snapshot(&state, limit_nodes.unwrap_or(400)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_graph_node_by_normalized_name(
    state: tauri::State<'_, SharedState>,
    normalized: String,
) -> Result<Option<Value>, String> {
    graph_index_service::get_node_by_normalized_name(&state, &normalized).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_graph_neighbors_two_hop(
    state: tauri::State<'_, SharedState>,
    node_id: String,
) -> Result<Value, String> {
    graph_index_service::get_neighbors_two_hop(&state, &node_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_graph_edges_for_document(
    state: tauri::State<'_, SharedState>,
    document_id: String,
) -> Result<Vec<Value>, String> {
    graph_index_service::get_edges_for_document(&state, &document_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_graph_edges_for_chunk(
    state: tauri::State<'_, SharedState>,
    chunk_id: String,
) -> Result<Vec<Value>, String> {
    graph_index_service::get_edges_for_chunk(&state, &chunk_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_graph_node_detail(
    state: tauri::State<'_, SharedState>,
    node_id: String,
) -> Result<Option<Value>, String> {
    graph_index_service::get_node_detail(&state, &node_id).map_err(|e| e.to_string())
}
