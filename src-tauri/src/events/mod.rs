//! Tauri event name constants.

pub const IMPORT_PROGRESS: &str = "import-progress";

pub const BACKGROUND_PROGRESS: &str = "background-progress";

pub const LIBRARY_CHANGED: &str = "library-changed";

#[allow(dead_code)] // reserved; grounded chat uses CHAT_STAGE / CHAT_FINAL
pub const CHAT_STREAM: &str = "chat-stream";
pub const CHAT_STAGE: &str = "chat-stage";
pub const CHAT_FINAL: &str = "chat-final";

pub const MODELS_PROGRESS: &str = "models-progress";
