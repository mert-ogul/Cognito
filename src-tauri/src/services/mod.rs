//! Small orchestration units for commands (no vague "manager" types).

pub mod chat_query;
pub mod file_watcher;
pub mod health_snapshot;
pub mod graph_index_service;
pub mod graph_worker;
pub mod ollama_stream;
pub mod import_job;
pub mod ingest;
pub mod model_warm;
pub mod watcher_action;
