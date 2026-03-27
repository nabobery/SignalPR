use std::sync::Arc;

use serde_json::Value;

use crate::errors::AppError;
use crate::providers::codex_app_server::manager::CodexAppServerManager;

#[tauri::command]
pub async fn resolve_codex_approval(
    request_id: Value,
    decision: String,
    manager: tauri::State<'_, Arc<CodexAppServerManager>>,
) -> Result<(), AppError> {
    manager
        .respond_to_approval(request_id, Value::String(decision))
        .await?;
    Ok(())
}
