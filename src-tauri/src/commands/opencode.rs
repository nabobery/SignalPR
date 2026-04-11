use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::opencode::manager::OpenCodeManager;

#[tauri::command]
pub async fn resolve_opencode_permission(
    request_id: String,
    reply: String,
    manager: tauri::State<'_, Arc<OpenCodeManager>>,
) -> Result<(), AppError> {
    manager.respond_to_permission(&request_id, &reply).await?;
    Ok(())
}
