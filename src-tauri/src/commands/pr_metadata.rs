use serde::Serialize;
use tauri::AppHandle;

use crate::errors::AppError;
use crate::platform::adapter::PlatformMetadata;
use crate::platform::{self, ParsedReviewUrl};
use crate::storage::db::AppDb;
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct RefreshMetadataResult {
    pub pr_id: String,
    pub fetched_at: String,
    pub metadata: PlatformMetadata,
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
    let adapter = build_adapter(&app, &review_url).await?;

    let metadata = adapter.fetch_metadata().await?;

    let json =
        serde_json::to_string(&metadata).map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let fetched_at_now = chrono::Utc::now().to_rfc3339();
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::update_pull_request_metadata(&conn, &pr_id, &json, &fetched_at_now)?;
    }

    let fetched_at = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::get_pull_request(&conn, &pr_id)?
            .and_then(|pr| pr.platform_metadata_fetched_at)
            .unwrap_or_default()
    };

    Ok(RefreshMetadataResult {
        pr_id,
        fetched_at,
        metadata,
    })
}

/// Build the appropriate platform adapter from a parsed review URL.
pub async fn build_adapter(
    app: &AppHandle,
    review_url: &ParsedReviewUrl,
) -> Result<Box<dyn crate::platform::adapter::PlatformAdapter>, AppError> {
    match review_url {
        ParsedReviewUrl::GitHub {
            owner,
            repo,
            number,
            ..
        } => {
            let (api, _source) = crate::providers::github::resolve_api_from_app(app).await?;
            Ok(Box::new(
                crate::platform::github_adapter::GitHubAdapter::new(
                    api,
                    owner.clone(),
                    repo.clone(),
                    *number,
                ),
            ))
        }
        ParsedReviewUrl::GitLab {
            host,
            project_path,
            iid,
        } => {
            let token = crate::providers::gitlab::resolve_gitlab_token().ok_or_else(|| {
                AppError::InvalidInput(
                    "GITLAB_TOKEN environment variable is not set. Set it to a GitLab personal access token with api scope.".into(),
                )
            })?;
            let api = crate::providers::gitlab::GitLabApi::new(token, host)
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            Ok(Box::new(
                crate::platform::gitlab_adapter::GitLabAdapter::new(
                    api,
                    host.clone(),
                    project_path.clone(),
                    *iid,
                ),
            ))
        }
        ParsedReviewUrl::Bitbucket {
            workspace,
            repo_slug,
            pull_request_id,
            ..
        } => {
            let credentials =
                crate::providers::bitbucket::resolve_bitbucket_credentials_from_env()
                    .ok_or_else(|| {
                        AppError::InvalidInput(
                            "Bitbucket credentials not set. Set BITBUCKET_EMAIL and BITBUCKET_TOKEN environment variables (API token, not app password).".into(),
                        )
                    })?;
            let api = crate::providers::bitbucket::BitbucketApi::try_new(credentials)
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            Ok(Box::new(
                crate::platform::bitbucket_adapter::BitbucketAdapter::new(
                    api,
                    workspace.clone(),
                    repo_slug.clone(),
                    *pull_request_id,
                ),
            ))
        }
    }
}
