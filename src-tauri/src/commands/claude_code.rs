use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::claude_code::manager::ClaudeCodeManager;

/// Resolve a pending Claude Code permission request from the frontend.
///
/// In v1 the Claude Code bridge auto-denies all non-allowlisted tool calls
/// and uses this IPC only for observability — the frontend listens to
/// `claude_code_permission_requested` events and can acknowledge them.
#[tauri::command]
pub async fn resolve_claude_code_permission(
    _request_id: String,
    _approved: bool,
    _manager: tauri::State<'_, Arc<ClaudeCodeManager>>,
) -> Result<(), AppError> {
    Ok(())
}
