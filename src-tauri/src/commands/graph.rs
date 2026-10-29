use crate::app::state::SharedState;
use crate::services::graph_index_service;
use serde_json::Value;

use super::run_blocking;

#[tauri::command]
pub async fn search_graph(
    state: tauri::State<'_, SharedState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    let lim = limit.unwrap_or(50);
    run_blocking(state, move |st| {
        graph_index_service::search_graph_nodes(&st, &query, lim)
    })
    .await
}

#[tauri::command]
pub async fn get_graph_snapshot(
    state: tauri::State<'_, SharedState>,
    limit_nodes: Option<usize>,
) -> Result<Value, String> {
    let lim = limit_nodes.unwrap_or(400);
    run_blocking(state, move |st| {
        graph_index_service::get_graph_snapshot(&st, lim)
    })
    .await
}

#[tauri::command]
pub async fn get_graph_node_detail(
    state: tauri::State<'_, SharedState>,
    node_id: String,
) -> Result<Option<Value>, String> {
    run_blocking(state, move |st| {
        graph_index_service::get_node_detail(&st, &node_id)
    })
    .await
}
