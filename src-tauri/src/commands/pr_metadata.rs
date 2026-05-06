use serde::Serialize;
use tauri::AppHandle;

use crate::commands::intake::parse_pr_url;
use crate::context_pack::extract_issue_refs;
use crate::errors::AppError;
use crate::providers::github::{
    resolve_api_from_app, GitHubApiError, PlatformMetadataSnapshot, ReviewStateSummary, TokenSource,
};
use crate::storage::db::AppDb;
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct RefreshMetadataResult {
    pub pr_id: String,
    pub fetched_at: String,
    pub metadata: PlatformMetadataSnapshot,
}

#[tauri::command]
pub async fn refresh_pr_metadata(
    app: AppHandle,
    pr_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<RefreshMetadataResult, AppError> {
    let (url, pr_number) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        (pr.url, pr.pr_number)
    };

    let parsed = parse_pr_url(&url)?;

    let (api, token_source) = resolve_api_from_app(&app).await?;
    tracing::debug!(
        "Refreshing GitHub metadata using token source: {}",
        match token_source {
            TokenSource::EnvVar => "env",
            TokenSource::GhCli => "gh_cli",
        }
    );

    let gh_pr = api
        .get_pull_request(&parsed.owner, &parsed.repo, pr_number)
        .await
        .map_err(gh_err_to_app)?;

    let reviewers = api
        .get_requested_reviewers(&parsed.owner, &parsed.repo, pr_number)
        .await
        .map_err(gh_err_to_app)?;

    let reviews = api
        .list_reviews(&parsed.owner, &parsed.repo, pr_number)
        .await
        .map_err(gh_err_to_app)?;

    let linked_issues = api
        .get_linked_issues(&parsed.owner, &parsed.repo, pr_number)
        .await
        .map_err(gh_err_to_app)?;

    let text_refs = gh_pr
        .body
        .as_deref()
        .map(extract_issue_refs)
        .unwrap_or_default();

    let title_refs = extract_issue_refs(&gh_pr.title);
    let mut all_text_refs = text_refs;
    for r in title_refs {
        if !all_text_refs.contains(&r) {
            all_text_refs.push(r);
        }
    }

    let snapshot = PlatformMetadataSnapshot {
        pr_body: gh_pr.body,
        head_sha: gh_pr.head.sha,
        base_sha: gh_pr.base.sha,
        base_ref: gh_pr.base.ref_name,
        head_ref: gh_pr.head.ref_name,
        draft: gh_pr.draft.unwrap_or(false),
        labels: gh_pr
            .labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.name)
            .collect(),
        requested_reviewers: reviewers.users.into_iter().map(|u| u.login).collect(),
        requested_teams: reviewers.teams.into_iter().map(|t| t.slug).collect(),
        review_state_summary: reviews
            .into_iter()
            .filter_map(|r| {
                Some(ReviewStateSummary {
                    login: r.user?.login,
                    state: r.state,
                    submitted_at: r.submitted_at,
                })
            })
            .collect(),
        linked_issue_numbers: linked_issues,
        text_issue_refs: all_text_refs,
    };

    let json =
        serde_json::to_string(&snapshot).map_err(|e| AppError::InvalidInput(e.to_string()))?;

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
        metadata: snapshot,
    })
}

fn gh_err_to_app(e: GitHubApiError) -> AppError {
    match e {
        GitHubApiError::RateLimited { .. } => AppError::InvalidInput(e.to_string()),
        GitHubApiError::HttpError { status: 404, .. } => AppError::NotFound(e.to_string()),
        _ => AppError::InvalidInput(e.to_string()),
    }
}
