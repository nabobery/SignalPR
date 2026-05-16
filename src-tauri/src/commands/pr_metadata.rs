use serde::Serialize;
use tauri::AppHandle;

use crate::errors::AppError;
use crate::platform;
use crate::platform::adapter::{PlatformCapabilities, PlatformMetadata};
use crate::storage::db::AppDb;
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct RefreshMetadataResult {
    pub pr_id: String,
    pub fetched_at: String,
    pub metadata: PlatformMetadata,
    pub capabilities: PlatformCapabilities,
    pub capabilities_fetched_at: String,
}

#[tauri::command]
pub async fn refresh_pr_metadata(
    app: AppHandle,
    pr_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<RefreshMetadataResult, AppError> {
    let url = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        pr.url
    };

    let review_url = platform::parse_review_url(&url)?;
    let adapter = crate::platform::factory::build_adapter(&app, &review_url).await?;

    let capabilities = adapter.capabilities();
    let metadata = adapter.fetch_metadata().await?;

    let json =
        serde_json::to_string(&metadata).map_err(|e| AppError::InvalidInput(e.to_string()))?;
    let capabilities_json =
        serde_json::to_string(&capabilities).map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let fetched_at_now = chrono::Utc::now().to_rfc3339();
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::update_pull_request_metadata(&conn, &pr_id, &json, &fetched_at_now)?;
        queries::update_pull_request_capabilities(
            &conn,
            &pr_id,
            &capabilities_json,
            &fetched_at_now,
        )?;
    }

    let (fetched_at, capabilities_fetched_at) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        (
            pr.platform_metadata_fetched_at.unwrap_or_default(),
            pr.platform_capabilities_fetched_at.unwrap_or_default(),
        )
    };

    Ok(RefreshMetadataResult {
        pr_id,
        fetched_at,
        metadata,
        capabilities,
        capabilities_fetched_at,
    })
}
