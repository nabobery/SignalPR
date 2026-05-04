use crate::storage::db::AppDb;
use crate::storage::models::ReviewDraft;
use crate::storage::queries;

#[tauri::command]
pub async fn get_review_draft(
    run_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<Option<ReviewDraft>, crate::errors::AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| crate::errors::AppError::InvalidInput(e.to_string()))?;
    let draft = queries::get_review_draft(&conn, &run_id)?;
    Ok(draft)
}

#[tauri::command]
pub async fn save_review_draft(
    run_id: String,
    summary_markdown: String,
    review_action: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), crate::errors::AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| crate::errors::AppError::InvalidInput(e.to_string()))?;
    queries::save_review_draft(&conn, &run_id, &summary_markdown, &review_action)?;
    Ok(())
}
