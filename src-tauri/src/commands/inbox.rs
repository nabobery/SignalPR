use serde::Serialize;
use tauri::AppHandle;

use crate::commands::environment::EnvironmentSummary;
use crate::storage::db::AppDb;
use crate::storage::models::{InboxReviewRow, InboxWorkspaceRow};
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct InboxOverview {
    pub environment_summary: EnvironmentSummary,
    pub incomplete_reviews: Vec<InboxReviewRow>,
    pub recent_reviews: Vec<InboxReviewRow>,
    pub recent_workspaces: Vec<InboxWorkspaceRow>,
}

#[tauri::command]
pub async fn get_inbox_overview(
    app: AppHandle,
    db: tauri::State<'_, AppDb>,
) -> Result<InboxOverview, crate::errors::AppError> {
    use crate::errors::AppError;

    let env_summary = crate::commands::environment::build_environment_summary(&app).await;

    let (incomplete, recent, recent_workspaces) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let incomplete = queries::list_incomplete_review_runs_enriched(&conn, 20)?;
        let recent = queries::list_recent_review_runs(&conn, 20)?;
        let recent_workspaces = queries::list_recent_workspaces(&conn, 8)?;
        (incomplete, recent, recent_workspaces)
    };

    Ok(InboxOverview {
        environment_summary: env_summary,
        incomplete_reviews: incomplete,
        recent_reviews: recent,
        recent_workspaces,
    })
}
