use tauri::AppHandle;

use crate::errors::AppError;

use super::adapter::PlatformAdapter;
use super::ParsedReviewUrl;

pub async fn build_adapter(
    app: &AppHandle,
    review_url: &ParsedReviewUrl,
) -> Result<Box<dyn PlatformAdapter>, AppError> {
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
