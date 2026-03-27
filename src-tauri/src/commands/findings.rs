use crate::errors::AppError;
use crate::storage::db::AppDb;
use crate::storage::queries;

#[tauri::command]
pub async fn update_finding(
    finding_id: String,
    body: Option<String>,
    severity: Option<String>,
    status: Option<String>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    queries::update_finding(
        &conn,
        &finding_id,
        body.as_deref(),
        severity.as_deref(),
        status.as_deref(),
    )?;
    Ok(())
}
