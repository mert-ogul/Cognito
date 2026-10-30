mod chat;
mod connections;
mod documents;
mod health;
mod models;
mod settings;
mod vault;

use crate::app::state::SharedState;
use crate::errors::CognitoError;

pub(crate) async fn run_blocking<T, F>(
    state: tauri::State<'_, SharedState>,
    f: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SharedState) -> Result<T, CognitoError> + Send + 'static,
{
    let st = (*state).clone();
    tauri::async_runtime::spawn_blocking(move || f(st))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

pub use chat::*;
pub use connections::*;
pub use documents::*;
pub use health::*;
pub use models::*;
pub use settings::*;
pub use vault::*;
