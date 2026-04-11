use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::copilot::manager::CopilotManager;

#[tauri::command]
pub async fn resolve_copilot_permission(
    session_id: String,
    event_id: String,
    decision: String,
    manager: tauri::State<'_, Arc<CopilotManager>>,
) -> Result<(), AppError> {
    manager
        .respond_to_permission(&session_id, &event_id, &decision)
        .await?;
    Ok(())
}
