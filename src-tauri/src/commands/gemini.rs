use std::sync::Arc;

use crate::errors::AppError;
use crate::providers::gemini::manager::GeminiManager;

/// Resolve a pending Gemini permission request from the frontend.
///
/// Note: in the current MVP, the Gemini manager auto-approves permission
/// requests server-side and uses this IPC only for observability (the
/// frontend listens to `gemini_permission_requested` events and can log
/// them). A future PR will gate the approval on the user's decision before
/// the manager sends the ACP response.
#[tauri::command]
pub async fn resolve_gemini_permission(
    _request_id: String,
    _approved: bool,
    _manager: tauri::State<'_, Arc<GeminiManager>>,
) -> Result<(), AppError> {
    // TODO(phase-2): route the decision back through the manager to gate the
    // ACP `session/request_permission` response on user input. For now the
    // manager auto-approves, so this handler is a no-op acknowledgement.
    Ok(())
}
