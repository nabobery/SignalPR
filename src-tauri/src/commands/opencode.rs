use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::capabilities::ToolGovernanceTier;
use crate::providers::governance;
use crate::providers::opencode::manager::OpenCodeManager;
use crate::storage::db::AppDb;
use crate::storage::queries;

#[tauri::command]
pub async fn resolve_opencode_permission(
    request_id: String,
    reply: String,
    manager: tauri::State<'_, Arc<OpenCodeManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    if reply == "allow" || reply == "always_allow" {
        let configured_tier = {
            let conn =
                db.0.lock()
                    .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            queries::get_setting(&conn, "opencode_governance_tier")?
                .and_then(|s| ToolGovernanceTier::from_str(&s))
        };
        governance::check_approval_allowed("opencode", configured_tier)?;
    }

    manager.respond_to_permission(&request_id, &reply).await?;
    Ok(())
}
