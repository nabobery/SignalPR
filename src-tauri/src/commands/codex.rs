use std::sync::Arc;

use serde_json::Value;

use crate::errors::AppError;
use crate::providers::capabilities::ToolGovernanceTier;
use crate::providers::codex_app_server::manager::CodexAppServerManager;
use crate::providers::governance;
use crate::storage::db::AppDb;
use crate::storage::queries;

#[tauri::command]
pub async fn resolve_codex_approval(
    request_id: Value,
    decision: String,
    manager: tauri::State<'_, Arc<CodexAppServerManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    if decision == "approve" || decision == "always_approve" {
        let configured_tier = {
            let conn =
                db.0.lock()
                    .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            queries::get_setting(&conn, "codex_app_server_governance_tier")?
                .and_then(|s| ToolGovernanceTier::from_str(&s))
        };
        governance::check_approval_allowed("codex_app_server", configured_tier)?;
    }

    manager
        .respond_to_approval(request_id, Value::String(decision))
        .await?;
    Ok(())
}
