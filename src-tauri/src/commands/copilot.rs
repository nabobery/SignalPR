use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::capabilities::ToolGovernanceTier;
use crate::providers::copilot::manager::CopilotManager;
use crate::providers::governance;
use crate::storage::db::AppDb;
use crate::storage::queries;

#[tauri::command]
pub async fn resolve_copilot_permission(
    session_id: String,
    event_id: String,
    decision: String,
    manager: tauri::State<'_, Arc<CopilotManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    if decision == "allow" || decision == "always_allow" {
        let configured_tier = {
            let conn =
                db.0.lock()
                    .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            queries::get_setting(&conn, "copilot_governance_tier")?
                .and_then(|s| ToolGovernanceTier::from_str(&s))
        };
        governance::check_approval_allowed("copilot", configured_tier)?;
    }

    manager
        .respond_to_permission(&session_id, &event_id, &decision)
        .await?;
    Ok(())
}
