use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::cursor::manager::CursorManager;

/// Resolve a pending Cursor permission request from the frontend.
///
/// Note: in the current MVP the Cursor manager denies every tool request
/// by default (selecting the agent's `reject_once` option before
/// broadcasting the attempt) and uses this IPC only for observability —
/// the frontend listens to `cursor_permission_requested` events and can
/// acknowledge them. A future PR will gate the ACP response on the user's
/// decision before the manager replies to the agent.
#[tauri::command]
pub async fn resolve_cursor_permission(
    _request_id: String,
    _approved: bool,
    _manager: tauri::State<'_, Arc<CursorManager>>,
) -> Result<(), AppError> {
    // TODO(phase-2): route the decision back through the manager to gate
    // the ACP `session/request_permission` response on user input. For
    // now the manager auto-denies, so this handler is a no-op
    // acknowledgement.
    Ok(())
}
