use std::path::Path;

use crate::autofix::apply;
use crate::errors::AppError;
use crate::storage::db::AppDb;
use crate::storage::queries;

/// Resolve the workspace local_path for a given finding by traversing:
/// finding → review_run → pull_request → workspace
fn resolve_workspace_path(
    conn: &rusqlite::Connection,
    finding_id: &str,
) -> Result<(crate::storage::models::Finding, String, Vec<String>), AppError> {
    let finding = queries::get_finding_by_id(conn, finding_id)?
        .ok_or_else(|| AppError::NotFound(format!("Finding not found: {}", finding_id)))?;

    let run = queries::get_review_run(conn, &finding.review_run_id)?.ok_or_else(|| {
        AppError::NotFound(format!("Review run not found: {}", finding.review_run_id))
    })?;

    let pr = queries::get_pull_request(conn, &run.pr_id)?
        .ok_or_else(|| AppError::NotFound(format!("Pull request not found: {}", run.pr_id)))?;

    let workspace = queries::get_workspace_by_id(conn, &pr.workspace_id)?
        .ok_or_else(|| AppError::NotFound(format!("Workspace not found: {}", pr.workspace_id)))?;

    let changed_files = pr
        .changed_files
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default();

    Ok((finding, workspace.local_path, changed_files))
}

/// Validate that a finding has fix_search and fix_replace populated, and return them
/// along with the file_path.
fn extract_fix_fields<'a>(
    finding: &'a crate::storage::models::Finding,
    changed_files: &[String],
) -> Result<(&'a str, &'a str, &'a str), AppError> {
    let file_path = finding
        .file_path
        .as_deref()
        .ok_or_else(|| AppError::InvalidInput("Finding has no file_path".to_string()))?;
    if !changed_files.is_empty() && !changed_files.iter().any(|p| p == file_path) {
        return Err(AppError::InvalidInput(format!(
            "Refusing to apply fix to non-PR file '{}'",
            file_path
        )));
    }
    let search = finding
        .fix_search
        .as_deref()
        .ok_or_else(|| AppError::InvalidInput("Finding has no fix_search".to_string()))?;
    let replace = finding
        .fix_replace
        .as_deref()
        .ok_or_else(|| AppError::InvalidInput("Finding has no fix_replace".to_string()))?;
    Ok((file_path, search, replace))
}

#[tauri::command]
pub async fn preview_fix(
    finding_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<String, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let (finding, workspace_path, changed_files) = resolve_workspace_path(&conn, &finding_id)?;
    let (file_path, search, replace) = extract_fix_fields(&finding, &changed_files)?;

    apply::preview_fix(Path::new(&workspace_path), file_path, search, replace)
}

#[tauri::command]
pub async fn apply_fix(finding_id: String, db: tauri::State<'_, AppDb>) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let (finding, workspace_path, changed_files) = resolve_workspace_path(&conn, &finding_id)?;
    let (file_path, search, replace) = extract_fix_fields(&finding, &changed_files)?;

    apply::apply_fix(Path::new(&workspace_path), file_path, search, replace)?;
    queries::update_finding_fix_status(&conn, &finding_id, "applied")?;
    Ok(())
}

#[tauri::command]
pub async fn accept_fix(finding_id: String, db: tauri::State<'_, AppDb>) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let (finding, workspace_path, changed_files) = resolve_workspace_path(&conn, &finding_id)?;
    let (file_path, search, replace) = extract_fix_fields(&finding, &changed_files)?;

    apply::apply_fix(Path::new(&workspace_path), file_path, search, replace)?;
    queries::update_finding_fix_status(&conn, &finding_id, "accepted")?;
    Ok(())
}

#[tauri::command]
pub async fn reject_fix(finding_id: String, db: tauri::State<'_, AppDb>) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    // Verify the finding exists
    queries::get_finding_by_id(&conn, &finding_id)?
        .ok_or_else(|| AppError::NotFound(format!("Finding not found: {}", finding_id)))?;

    queries::update_finding_fix_status(&conn, &finding_id, "rejected")?;
    Ok(())
}
