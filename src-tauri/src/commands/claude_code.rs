use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::capabilities::ToolGovernanceTier;
use crate::providers::claude_code::manager::ClaudeCodeManager;
use crate::providers::governance;
use crate::storage::db::AppDb;
use crate::storage::queries;

/// Resolve a pending Claude Code permission request from the frontend.
///
/// In v1 with default `read_only` tier, this returns an error to prevent
/// tool approval. When the user opts into `guarded_write` via settings,
/// the approval is forwarded to the manager to unblock the sidecar.
#[tauri::command]
pub async fn resolve_claude_code_permission(
    request_id: String,
    approved: bool,
    manager: tauri::State<'_, Arc<ClaudeCodeManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    let configured_tier = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::get_setting(&conn, "claude_code_governance_tier")?
            .and_then(|s| ToolGovernanceTier::from_str(&s))
    };

    if approved {
        governance::check_approval_allowed("claude_code", configured_tier)?;
    }

    manager.resolve_permission(&request_id, approved).await;
    Ok(())
}
